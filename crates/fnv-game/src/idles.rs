//! Idle animations asked for by the game's rules (Layer 6): a script's `PlayIdle`, a
//! package's begin / end / change idles and its idle collection, an idle marker's
//! collection, and the idles named on a dialogue response. Facts and their sources:
//! docs/spec-notes/idles.md.
//!
//! The rules only say *which* idle an actor is to play and why ([`IdleRequest`], taken
//! from [`Game::idle_requests`] by whoever animates actors, and traced as
//! `idle_asked`). Playing it (the `.kf` file, its length, its loops) is the animating
//! layer's work, which tells the rules what an actor plays ([`Game::set_idle_playing`])
//! so `IsIdlePlaying` can answer. With nothing animating, no idle is ever playing.

use crate::cond::Condition;
use crate::data::IdleDef;
use crate::functions::{none, yes, Args, Ctx};
use crate::game::Game;
use crate::package::IdleCollection;
use crate::trace::EventKind;
use fnv_script::vm::{ArgValue, Value};
use fnv_sim::FormId;
use serde::Serialize;

/// Why an idle is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleSource {
    /// `PlayIdle` in a script.
    Script,
    /// The package's Begin, End or Change idle (PACK `INAM` after POBA / POEA / POCA).
    PackageBegin,
    PackageEnd,
    PackageChange,
    /// An idle of the package's idle collection (IDLA).
    PackageIdles,
    /// An idle of the collection of the idle marker the actor stands at.
    MarkerIdles,
    /// The speaker's or the listener's idle of a dialogue response (INFO `SNAM`, `LNAM`).
    Speaker,
    Listener,
    /// A drawn-weapon fidget picked from the idle tree ([`Game::fidget`]).
    Fidget,
    /// A fidget of a person standing with its weapon away, picked from the idle tree
    /// (biting its nails, smoking, drinking while it eats; [`FidgetKind::WeaponAway`]).
    HolsteredFidget,
    /// A fidget on furniture (sitting or lying) picked from the idle tree.
    FurnitureFidget,
    /// A creature standing alerted plays a combat idle from the idle tree (the gecko's
    /// hiss; [`FidgetKind::Alerted`]).
    CombatIdle,
    /// A creature standing still and calm plays an idle from the idle tree (a
    /// protectron's look about; [`FidgetKind::Calm`]).
    CalmIdle,
    /// A talking or listening gesture picked from the idle tree
    /// ([`Game::talk_gesture`]).
    Gesture,
    /// A fleeing actor standing still cowers: an idle at the root of the tree that
    /// wants the flee procedure ([`Game::flee_idle`]).
    Cower,
    /// A script's `PickIdle`: the idle tree's pick by its conditions ([`Game::pick_idle`]).
    Picked,
    /// A creature standing still plays a look of the idle tree's Idle section in place
    /// of its base idle, looping while its branch holds ([`Game::standing_idle`]).
    Standing,
    /// An NPC uses a stimpak it carries (`crate::npc_stimpaks`): played on its upper
    /// body only, over what it does otherwise.
    Stimpak,
    /// An actor picks up a weapon or ammunition it fetched (`crate::rearm`).
    PickUp,
    /// An actor with a live explosive planted in its pocket panics ([`Game::panic_idle`]).
    Panic,
    /// The player activated something and the idle tree's activation root had an idle
    /// for it: drinking from placeable water ([`Game::activate_idle`], #2349).
    Activate,
}

/// Which fidgets of the idle tree are played: the branch below a node whose conditions
/// require them (the rest of the tree is not built).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FidgetKind {
    /// Standing with the weapon out: below a node requiring `IsWeaponOut == 1`
    /// (idles.md §11).
    WeaponOut,
    /// A person standing with the weapon away: below a node requiring `IsWeaponOut == 0`
    /// or `IsWeaponOut != 1`, not joined by OR (idles.md §11.4, #2026) [observed:
    /// FalloutNV.esm's people's tree has two such branches walked for NPCs: the general
    /// fidgets, smoking and the Pip-Boy look below one node under the NPC-only node, and
    /// the drinking idles of the root that wants the eat procedure; their own conditions
    /// decide the rest: factions, random percentages, worn items]. Not asked for the
    /// player [design: the player's fidgets standing are the weapon-out ones, owner
    /// 2026-10-16].
    WeaponAway,
    /// On furniture: below a node requiring `GetSitting == 3` or `GetSleeping == 3`, the
    /// state of being on it (furniture.md §8).
    OnFurniture,
    /// A creature standing alerted (a fight, or a warning, `Game::warning_to`): below a
    /// node that wants `GetIsAlerted == 1`, alone or joined by OR (NVGeckoCombatIdles:
    /// IsMoving != 1 and GetIsAlerted 1 or the combat or flee procedure; MWarning,
    /// StBWarning) [observed: FalloutNV.esm; idles.md §14].
    Alerted,
    /// A creature standing still and calm (not alerted, not on furniture): no node gates
    /// it, the tree's own conditions decide [observed: HELIOS One's stasis protectrons,
    /// calm on ProtectronNotActive, play the protectron tree's CommonIdle leaves
    /// IdleSearch, IdleLookLeft and IdleLookRight, #1977 probe 2, 2026-11-10]. The
    /// Idle-section leaves are the standing looks' ([`Game::standing_idle`]): the engine
    /// holds them back.
    Calm,
}

impl FidgetKind {
    /// Whether idle `idle`'s own conditions make the branch below it this kind's.
    fn gates(self, data: &crate::data::GameData, idle: FormId) -> bool {
        self.gated_by(data.idles.get(&idle).and_then(|d| d.conditions.as_deref()).unwrap_or_default())
    }

    fn gated_by(self, conds: &[Condition]) -> bool {
        match self {
            FidgetKind::WeaponOut => requires(conds, IS_WEAPON_OUT, 1.0),
            FidgetKind::WeaponAway => requires(conds, IS_WEAPON_OUT, 0.0) || requires_not(conds, IS_WEAPON_OUT, 1.0),
            FidgetKind::OnFurniture => requires(conds, GET_SITTING, 3.0) || requires(conds, GET_SLEEPING, 3.0),
            FidgetKind::Alerted => names(conds, GET_IS_ALERTED, 1.0),
            FidgetKind::Calm => true,
        }
    }

    pub fn source(self) -> IdleSource {
        match self {
            FidgetKind::WeaponOut => IdleSource::Fidget,
            FidgetKind::WeaponAway => IdleSource::HolsteredFidget,
            FidgetKind::OnFurniture => IdleSource::FurnitureFidget,
            FidgetKind::Alerted => IdleSource::CombatIdle,
            FidgetKind::Calm => IdleSource::CalmIdle,
        }
    }
}

impl IdleSource {
    /// Asked for by a package or a marker: such an idle ends with what asked for it
    /// ([`IdleRequest`] users: a looping one leaves its loop when the package changes).
    pub fn of_package(self) -> bool {
        matches!(self, IdleSource::PackageBegin | IdleSource::PackageEnd | IdleSource::PackageChange | IdleSource::PackageIdles | IdleSource::MarkerIdles)
    }
}

/// Explicit player point-of-view override for `ForcePlayIdle` / `ForceStopIdle`
/// (`playerPOV`: `1` third-person, `2` first-person) [documented: GECK wiki ForcePlayIdle,
/// ForceStopIdle].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdlePov {
    Third,
    First,
}

/// An actor is to play an idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IdleRequest {
    pub actor: FormId,
    pub idle: FormId,
    pub source: IdleSource,
    /// Optional explicit player rig target (`ForcePlayIdle` arg 2); `None` preserves
    /// vanilla auto-routing by `Camera1st` track presence.
    pub pov: Option<IdlePov>,
}

/// How often an idle's loop plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Loops {
    /// Until something else replaces the idle.
    Forever,
    Times(u32),
}

impl IdleDef {
    /// How often the idle plays, from its loop minimum and maximum.
    ///
    /// - Both 255: the editor's "Loop Forever", until something replaces the idle
    ///   [observed: ticking Loop Forever in the GECK sets both to 255, and
    ///   `LooseVCG01PlayerWakeup` has it ticked; owner, 2026-10-10, idles.md §3]. A
    ///   maximum of 255 with another minimum is an ordinary range.
    /// - Otherwise a number of times from the minimum to the maximum [documented: GECK
    ///   wiki, Idle Animations Window], `roll(n)` choosing one of `n`; at least once, so
    ///   both 0 play the idle once [hypothesis].
    pub fn loops(&self, mut roll: impl FnMut(u32) -> u32) -> Loops {
        if self.loop_min == 255 && self.loop_max == 255 {
            return Loops::Forever;
        }
        let (lo, hi) = (self.loop_min.min(self.loop_max) as u32, self.loop_max as u32);
        Loops::Times((lo + if hi > lo { roll(hi - lo + 1) } else { 0 }).max(1))
    }
}

/// Where an actor is in an idle collection [documented: GECK wiki, Idle Collection].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IdlePicker {
    /// The next entry of a sequence.
    at: usize,
    /// Idles picked so far.
    picked: u32,
    done: bool,
}

impl IdlePicker {
    /// The next idle of `c` and the seconds to wait before it, once the last one has
    /// ended; `None` when the collection has nothing more to play. `roll(n)` chooses one
    /// of `n`.
    ///
    /// - Random: any listed idle, the idle timer between two idles.
    /// - Run In Sequence: from the top, the idle timer between whole sequences.
    /// - Do Once: the sequence plays once instead of starting over [documented for
    ///   sequences; hypothesis: a random collection then plays one idle].
    /// - The first idle is not waited for [hypothesis: the timer is documented as the
    ///   wait "before a new pick"].
    pub fn next(&mut self, c: &IdleCollection, mut roll: impl FnMut(usize) -> usize) -> Option<(FormId, f32)> {
        if self.done || c.idles.is_empty() {
            return None;
        }
        let timer = c.timer.max(0.0);
        let (idle, wait) = if c.run_in_sequence {
            let i = self.at.min(c.idles.len() - 1);
            self.at = i + 1;
            if self.at >= c.idles.len() {
                self.at = 0;
                self.done = c.do_once;
            }
            (c.idles[i], if i == 0 && self.picked > 0 { timer } else { 0.0 })
        } else {
            self.done = c.do_once;
            (c.idles[roll(c.idles.len()).min(c.idles.len() - 1)], if self.picked > 0 { timer } else { 0.0 })
        };
        self.picked += 1;
        Some((idle, wait))
    }
}

impl<'a> Game<'a> {
    /// The authored IDLE definition for `idle`, resolving `TempCloneForm`
    /// aliases back to their original IDLE record.
    pub fn idle_def(&self, idle: FormId) -> Option<&'a IdleDef> {
        let origin = self.unclone(idle);
        self.data.idles.get(&origin)
    }

    /// The current model file of an IDLE (or its `TempCloneForm` alias),
    /// including `SetIdleAnimPath` / `SetModelPath` session replacements snapshotted
    /// at `TempCloneForm` creation time. The renderer must use the same path as script
    /// getters [observed: `w4g_13_isidleplaying_and_idle_replace`].
    pub fn idle_animation_path(&self, idle: FormId) -> Option<&str> {
        let def = self.idle_def(idle)?;
        if let Some(fnv_script::vm::XVal::Str(path)) = self.field_overrides.get(&(idle, "MODL/".into())) {
            return Some(path.as_str());
        }
        def.model.as_deref()
    }

    /// Whether `idle` (or its `TempCloneForm` alias) currently names a `.kf`
    /// animation file, accounting for `SetIdleAnimPath` overrides.
    pub fn idle_has_file(&self, idle: FormId) -> bool {
        self.idle_animation_path(idle).is_some_and(|m| m.trim().to_ascii_lowercase().ends_with(".kf"))
    }

    /// `actor` is to play `idle`: queued for the animating layer and traced. An IDLE
    /// that names no animation file (an inner node of the idle tree) is not played
    /// [hypothesis: only idles with a file can be listed or played by name; the editor
    /// lists only those in collections, documented], which is traced instead.
    pub fn ask_idle(&mut self, actor: FormId, idle: FormId, source: IdleSource) -> bool {
        self.ask_idle_pov(actor, idle, source, None)
    }

    /// Queue `idle` on `actor` with an optional explicit player view override (`ForcePlayIdle`).
    pub fn ask_idle_pov(&mut self, actor: FormId, idle: FormId, source: IdleSource, pov: Option<IdlePov>) -> bool {
        let reason = match self.idle_def(idle) {
            None => Some("not an idle animation"),
            Some(_) if !self.idle_has_file(idle) => Some("names no animation file"),
            Some(_) => None,
        };
        if let Some(reason) = reason {
            self.emit(EventKind::IdleNotPlayed { actor, idle, source, reason });
            return false;
        }
        self.emit(EventKind::IdleAsked { actor, idle, source });
        self.idle_requests.push(IdleRequest { actor, idle, source, pov });
        true
    }

    /// Whether the idle's own conditions let `actor` play it now. An idle of a
    /// collection is played only then [hypothesis: Doc Mitchell's packages list the
    /// smoking idle, whose conditions he does not meet (a faction, a random percentage),
    /// and he does not smoke; idles.md §5]. An idle with a condition that could not be
    /// read is not allowed [design: what cannot be judged is not chosen].
    pub fn idle_allowed(&mut self, actor: FormId, idle: FormId) -> bool {
        self.idle_allowed_with(actor, None, idle)
    }

    /// As [`Game::idle_allowed`], with `target` as the conditions' target (Run On
    /// Target): the one the actor talks with, for the talking gestures.
    fn idle_allowed_with(&mut self, actor: FormId, target: Option<FormId>, idle: FormId) -> bool {
        match self.idle_def(idle).map(|d| d.conditions.as_deref()) {
            Some(Some(c)) => c.is_empty() || self.conditions_pass(c, Some(actor), target, &format!("IDLE {idle}")),
            _ => false,
        }
    }

    /// The animating layer says what special idle `actor` plays now (`None`: none).
    /// An idle told is also the actor's last idle played (IsLastIdlePlayed).
    pub fn set_idle_playing(&mut self, actor: FormId, idle: Option<FormId>) {
        if idle.is_some() || self.extra.contains_key(&actor) {
            let e = self.extra.entry(actor).or_default();
            e.idle = idle;
            if idle.is_some() {
                e.last_idle = idle;
            }
        }
    }

    /// The idle `actor` plays, as last told ([`Game::set_idle_playing`]).
    pub fn idle_playing(&self, actor: FormId) -> Option<FormId> {
        self.extra.get(&actor).and_then(|e| e.idle)
    }

    /// The actor's current package and its idle collection, when it has one with idles
    /// in it (the package the actor last chose; nothing is evaluated).
    pub fn package_idles(&self, actor: FormId) -> Option<(FormId, &'a IdleCollection)> {
        let p = self.extra.get(&actor)?.current_package?;
        let c = self.package_def(&p)?.idles.as_ref()?;
        (!c.idles.is_empty()).then_some((p, c))
    }

    /// A fidget of `kind` for `actor` from the idle tree, or `None`. `folder` is the
    /// actor's animation folder below `meshes\` (`characters\_male`); `held_back` says
    /// which idles are still within their replay delay. The rules
    /// (docs/spec-notes/idles.md §11, furniture.md §8):
    ///
    /// - Only the branches of the actor's own folder: an inner node's `MODL` names the
    ///   folder its idles are for (`Characters\_Male\IdleAnims`), and the creatures'
    ///   branches share the roots with the people's [observed; the folder match is a
    ///   hypothesis].
    /// - An NPC: from the roots in chain order, the first node whose conditions pass is
    ///   gone into, and the first leaf found that way is the pick; a node whose children
    ///   all fail is left for its next sibling [hypothesis: animation.md §2.2]. Roots
    ///   without children are idles played on request and are not walked. The pick is
    ///   kept only when a node on its way, the leaf included, gates `kind`
    ///   ([`FidgetKind`]): the rest of the tree is not built [design].
    /// - An NPC with its weapon away ([`FidgetKind::WeaponAway`]) walks the same way: the
    ///   people's tree gates its general fidgets, smoking and Pip-Boy look, and its
    ///   drinking idles while eating, by a node that wants the weapon away, and their
    ///   factions and random percentages are the nodes' own conditions (#2026).
    /// - The player on furniture walks the same way: the furniture branch has the
    ///   player's own nodes (`PlayerSittingChairIdles`, third person only) [observed].
    /// - The player with a weapon out [design, owner 2026-10-16: fidgets for the player
    ///   standing still]: the tree gives the player none (the people's weapon branch is
    ///   under an NPC-only node, the player's under a node that wants the weapon away),
    ///   so the walk starts at each node that requires the weapon out, its own
    ///   conditions and those below it judged, those above it not.
    ///
    /// The weapon-away walks ([`FidgetKind::WeaponAway`], asked of every NPC standing
    /// holstered) draw `GetRandomPercent` from the idle tree's own generator, as the
    /// talking gestures do, so they do not move the scripts' random numbers [engineering
    /// choice, #2026]. The other kinds still draw from the scripts' generator.
    pub fn fidget(&mut self, actor: FormId, folder: &str, kind: FidgetKind, held_back: &dyn Fn(FormId) -> bool) -> Option<FormId> {
        if kind != FidgetKind::WeaponAway {
            return self.fidget_walk(actor, folder, kind, held_back);
        }
        // The player standing with the weapon away fidgets not.
        if actor == FormId::PLAYER_REF {
            return None;
        }
        std::mem::swap(&mut self.rng, &mut self.idle_rng);
        let pick = self.fidget_walk(actor, folder, kind, held_back);
        std::mem::swap(&mut self.rng, &mut self.idle_rng);
        pick
    }

    /// Whether what `actor` does now has idles of its own, or none, so it gets no fidget
    /// with the weapon away and one playing ends (#2026) [design]: on furniture (getting
    /// on, on it or getting off: the furniture's fidgets are its own, furniture.md §8),
    /// its current package has an idle collection (whose idles play instead, §5), or its
    /// current package is flagged No Idle Anims ("Turns off Idle animations during
    /// package" [documented: GECK wiki, Category:Packages]). An idle marker's collection
    /// is the animating layer's to tell.
    pub fn fidget_held_off(&self, actor: FormId) -> bool {
        let quiet = self.extra.get(&actor).and_then(|e| e.current_package).and_then(|p| self.package_def(&p)).is_some_and(|d| d.has(crate::package::flags::NO_IDLE_ANIMS));
        self.furniture_use(actor).is_some() || self.package_idles(actor).is_some() || quiet
    }

    fn fidget_walk(&mut self, actor: FormId, folder: &str, kind: FidgetKind, held_back: &dyn Fn(FormId) -> bool) -> Option<FormId> {
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let own = |id: &FormId| data.idles.get(id).and_then(|d| d.model.as_deref()).is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder));
        let roots: Vec<FormId> = data.idle_children.get(&None).into_iter().flatten().copied().filter(|r| own(r) && data.idle_children.contains_key(&Some(*r))).collect();
        if actor != FormId::PLAYER_REF || kind == FidgetKind::OnFurniture {
            for root in roots {
                if !self.idle_allowed(actor, root) {
                    continue;
                }
                if let Some((leaf, gated)) = self.first_leaf(actor, root, Some(kind), kind.gates(data, root), held_back) {
                    return gated.then_some(leaf);
                }
            }
            return None;
        }
        let mut starts = Vec::new();
        let mut stack: Vec<FormId> = roots;
        while let Some(n) = stack.pop() {
            let kids = data.idle_children.get(&Some(n)).map(Vec::as_slice).unwrap_or_default();
            if data.idles[&n].parent.is_some() && kind.gates(data, n) && !kids.is_empty() {
                starts.push(n);
            } else {
                stack.extend(kids.iter().rev());
            }
        }
        starts.into_iter().find_map(|s| self.idle_allowed(actor, s).then(|| self.first_leaf(actor, s, Some(kind), true, held_back)).flatten().map(|(leaf, _)| leaf))
    }

    /// The idle a fleeing `actor` plays from the idle tree now, or `None` (idles.md §13).
    /// The tree's own roots that are idles (a file, no children) and whose conditions
    /// name the flee procedure (`GetCurrentAIProcedure == 16`, alone or among ORs) are
    /// tried in chain order, and the first whose conditions the actor meets is the pick
    /// [observed: FalloutNV.esm has two such roots, `NVKimbalCower` (no file) and
    /// `CowerIdles` 0x0006CB4A, `3rdP_Cowering.kf`, which wants IsMoving 0, the
    /// procedure 16 FLEE or 34 FLEE_NON_COMBAT and IsSwimming 0; hypothesis: the
    /// original's idle manager plays such a root by its conditions, as the roots of
    /// the fidget branches]. `folder` and `held_back` as for [`Game::fidget`].
    pub fn flee_idle(&mut self, actor: FormId, folder: &str, held_back: &dyn Fn(FormId) -> bool) -> Option<FormId> {
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let roots: Vec<FormId> = data
            .idle_children
            .get(&None)
            .into_iter()
            .flatten()
            .copied()
            .filter(|r| !data.idle_children.contains_key(&Some(*r)))
            .filter(|r| {
                data.idles.get(r).is_some_and(|d| {
                    d.has_file()
                        && d.model.as_deref().is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder))
                        && names(d.conditions.as_deref().unwrap_or_default(), GET_CURRENT_AI_PROCEDURE, FLEE_PROCEDURE)
                })
            })
            .collect();
        roots.into_iter().find(|r| !held_back(*r) && self.idle_allowed(actor, *r))
    }

    /// The panic idle of `actor`, with a live explosive in its pocket, from the idle tree,
    /// or `None` (docs/spec-notes/pickpocket.md §5): the roots of the actor's own `folder`
    /// in chain order whose conditions name `GetPlantedExplosive == 1`, gone into as for
    /// [`Game::pick_idle`] (the first leaf with a file whose conditions pass; a root that
    /// is itself such an idle is the pick when its conditions pass) [observed:
    /// FalloutNV.esm has one, `PanicExplosivePlanted` 0x00042F24 under
    /// `Characters\_Male\IdleAnims`, whose leaves are `PanicGrenadePlantPA` (GetEquipped
    /// on a form list, the power armour one) and `PanicGrenadePlant`; no creature folder
    /// has one, so a creature plays none and only stands; hypothesis: the original's idle
    /// manager plays it by its conditions while GetPlantedExplosive holds].
    pub fn panic_idle(&mut self, actor: FormId, folder: &str, held_back: &dyn Fn(FormId) -> bool) -> Option<FormId> {
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let roots: Vec<FormId> = data
            .idle_children
            .get(&None)
            .into_iter()
            .flatten()
            .copied()
            .filter(|r| {
                data.idles.get(r).is_some_and(|d| d.model.as_deref().is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder)) && names(d.conditions.as_deref().unwrap_or_default(), GET_PLANTED_EXPLOSIVE, 1.0))
            })
            .collect();
        for root in roots {
            if !self.idle_allowed(actor, root) {
                continue;
            }
            if !data.idle_children.contains_key(&Some(root)) {
                if data.idles[&root].has_file() && !held_back(root) {
                    return Some(root);
                }
                continue;
            }
            if let Some((leaf, _)) = self.first_leaf(actor, root, None, true, held_back) {
                return Some(leaf);
            }
        }
        None
    }

    /// The idle `actor` plays for activating something, from the idle tree, and the way
    /// to it (the root first, the leaf last), or `None` (docs/spec-notes/idles.md §16,
    /// #2349): the roots of the actor's own `folder` in chain order whose conditions name
    /// `GetUsedItemActivate == 1`, gone into as for [`Game::pick_idle`] (the first leaf
    /// with a file whose way down passes) [documented: fnvtool esm decode and actor anims,
    /// FalloutNV.esm has one for people,
    /// `ActivateIdles` 0x00003BDC, whose children are `ActStimpakSource` (GetIsUsedItem
    /// Stimpak), `ActWaterSource` 0x000221B4 (IsWaterObject == 1: `1stPersonActWater`,
    /// GetIsID Player and IsPC1stPerson, then `3rdPersonActWater` with no conditions) and
    /// `NPCPickUpItem` (not the player); documented: GECK wiki IsWaterObject, "Used in
    /// vanilla game for drinking animation"; hypothesis: the original asks this root when
    /// the player activates something]. During this walk, and only here,
    /// GetUsedItemActivate answers 1 while the actor activates something
    /// ([`Game::activating`]; elsewhere its answer is unchanged, #2349); IsWaterObject
    /// answers for what it activates; the tree's own conditions choose. `held_back` as for
    /// [`Game::fidget`].
    pub fn activate_idle(&mut self, actor: FormId, folder: &str, held_back: &dyn Fn(FormId) -> bool) -> Option<(FormId, Vec<FormId>)> {
        let before = self.activate_walk.replace(actor);
        // Put back even when the walk panics (a caller's `held_back`), so a caller that
        // catches it does not leave GetUsedItemActivate answering for the activation;
        // the panic goes on.
        let pick = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.activate_walk_tree(actor, folder, held_back)));
        self.activate_walk = before;
        pick.unwrap_or_else(|p| std::panic::resume_unwind(p))
    }

    /// [`Game::activate_idle`]'s walk, with the activation known to GetUsedItemActivate.
    fn activate_walk_tree(&mut self, actor: FormId, folder: &str, held_back: &dyn Fn(FormId) -> bool) -> Option<(FormId, Vec<FormId>)> {
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let roots: Vec<FormId> = data
            .idle_children
            .get(&None)
            .into_iter()
            .flatten()
            .copied()
            .filter(|r| data.idle_children.contains_key(&Some(*r)))
            .filter(|r| {
                data.idles.get(r).is_some_and(|d| d.model.as_deref().is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder)) && names(d.conditions.as_deref().unwrap_or_default(), GET_USED_ITEM_ACTIVATE, 1.0))
            })
            .collect();
        for root in roots {
            if !self.idle_allowed(actor, root) {
                continue;
            }
            if let Some((leaf, _)) = self.first_leaf(actor, root, None, true, held_back) {
                let mut way = vec![leaf];
                let mut at = leaf;
                while at != root {
                    let Some(up) = data.idles.get(&at).and_then(|d| d.parent) else { break };
                    way.push(up);
                    at = up;
                }
                way.reverse();
                return Some((leaf, way));
            }
        }
        None
    }

    /// The idle `actor` plays for a script's `PickIdle`, or `None`: the idle tree walked
    /// as for an NPC's fidget ([`Game::fidget`]: the roots of the actor's own `folder` in
    /// chain order, the first node whose conditions pass gone into, the first leaf with
    /// a file whose conditions pass is the pick), with no branch kept out
    /// [documented: GECK wiki PickIdle, "causes the calling actor to choose and play an
    /// Idle Animation", and "will do nothing if no Idle Animations can be found with
    /// matching conditions"; hypothesis: the walk is the fidgets' (animation.md §2.2),
    /// on the player too]. `held_back` as for [`Game::fidget`].
    pub fn pick_idle(&mut self, actor: FormId, folder: &str, held_back: &dyn Fn(FormId) -> bool) -> Option<FormId> {
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let own = |id: &FormId| data.idles.get(id).and_then(|d| d.model.as_deref()).is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder));
        let roots: Vec<FormId> = data.idle_children.get(&None).into_iter().flatten().copied().filter(|r| own(r) && data.idle_children.contains_key(&Some(*r))).collect();
        for root in roots {
            if !self.idle_allowed(actor, root) {
                continue;
            }
            if let Some((leaf, _)) = self.first_leaf(actor, root, None, true, held_back) {
                return Some(leaf);
            }
        }
        None
    }

    /// The powered-down pose of an actor that SetUnconscious holds standing (not one
    /// knocked out, which lies down: `crate::unconscious`), or `None`: the first idle of
    /// its own `folder`'s tree, in chain order, that names a file, tests GetUnconscious
    /// and whose way down passes [observed: FalloutNV.esm has one such leaf per body,
    /// `ProtectronTurnedOff` 0x0005B344 `MTSpecialIdle_Deactivate.kf`, `MrGutsyUnconscious`
    /// 0x00060C89 `SpecialIdle_Deactivate.kf`, `SentryBotUnconscious`,
    /// `RobobrainUnconscious` and more, each with the one condition GetUnconscious == 1;
    /// the protectron and Gutsy clips hold the switched-off pose between their StartLoop
    /// and EndLoop keys. Owner 2026-11-09, with a still of the original: HELIOS One's
    /// stasis protectrons and PYTHON stand arms down until switched on; documented: GECK
    /// wiki SetUnconscious, unconscious actors "will not use AI behavior"]. Played as the
    /// standing look ([`Game::standing_idle`]), looping until SetUnconscious 0 ends it.
    pub fn unconscious_idle(&mut self, actor: FormId, folder: &str) -> Option<FormId> {
        if !self.extra.get(&actor).is_some_and(|e| e.unconscious && e.knocked_out.is_none() && !e.dead) {
            return None;
        }
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let own = |id: &FormId| data.idles.get(id).and_then(|d| d.model.as_deref()).is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder));
        let off = |id: FormId| data.idles.get(&id).is_some_and(|d| d.has_file() && d.conditions.as_deref().is_some_and(|c| c.iter().any(|c| c.function == GET_UNCONSCIOUS)));
        let roots: Vec<FormId> = data.idle_children.get(&None).into_iter().flatten().copied().filter(own).collect();
        for root in roots {
            let branch = data.idle_children.contains_key(&Some(root));
            if (!branch && !off(root)) || !self.idle_allowed(actor, root) {
                continue;
            }
            if !branch {
                return Some(root);
            }
            if let Some((leaf, _)) = self.first_leaf(actor, root, None, true, &|i| !off(i)) {
                return Some(leaf);
            }
        }
        None
    }

    /// The standing look of creature `actor` from the idle tree, or `None`: the roots of
    /// the actor's own `folder` in chain order, as for [`Game::pick_idle`], counting only
    /// idles of the Idle section (DATA section 0, the `DynamicIdle_…` files) that are not
    /// Loose (`IdleDef::loose`): a branch whose conditions pass is gone into for its first
    /// such leaf whose conditions pass; a root that is such an idle itself is the pick when
    /// its own conditions pass [observed: FalloutNV.esm and the DLC masters have eight
    /// such creature leaves, each below a branch whose conditions name a state (Mr.
    /// House's screen, Festus switched off, the spore carriers unconscious, Mr. House's
    /// pod closed, Big MT's brain tanks closed), and one such root, the screen's
    /// `LooseNVPHMCconnectionLost`. The screen's roots in chain order: `NVPHMCYesMan`
    /// (GetStage VMQYesMan01 >= 70: Yes Man's face), `LooseNVPHMCconnectionLost`
    /// (GetGlobalValue 0x00136FCE > 0 or GetPCEnemyOfFaction 0x00126DAB == 1: "Connection
    /// Lost"), `NVPHMCHouse` (stage < 70, the global <= 0, not an enemy: House's face).
    /// Owner 2026-11-05: Connection Lost shows after House is killed and before Yes Man
    /// is uploaded, and in the Fort's bunker while House is not connected; hypothesis:
    /// the original plays such an idle in place of the standing idle while it holds]. The
    /// engine plays the pick looping and asks again now and then, so a change of state
    /// changes the look (#1542).
    pub fn standing_idle(&mut self, actor: FormId, folder: &str) -> Option<FormId> {
        if let Some(off) = self.unconscious_idle(actor, folder) {
            return Some(off);
        }
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let look = |id: FormId| data.idles.get(&id).is_some_and(|d| d.section == 0 && !d.loose && d.has_file());
        let own = |id: &FormId| data.idles.get(id).and_then(|d| d.model.as_deref()).is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder));
        let roots: Vec<FormId> = data.idle_children.get(&None).into_iter().flatten().copied().filter(own).collect();
        for root in roots {
            let branch = data.idle_children.contains_key(&Some(root));
            if !branch && !look(root) {
                continue;
            }
            if !self.idle_allowed(actor, root) {
                continue;
            }
            if !branch {
                return Some(root);
            }
            if let Some((leaf, _)) = self.first_leaf(actor, root, None, true, &|i| !look(i)) {
                return Some(leaf);
            }
        }
        None
    }

    /// The first leaf below `node` whose way down passes, and whether a node on that
    /// way (or `gated`, said of those above) gates `kind`.
    /// PlayIdleEx's pick under a parent idle (`crate::nvse_jip14`): its first leaf whose
    /// conditions, and its branches', pass.
    pub(crate) fn first_idle_leaf(&mut self, actor: FormId, parent: FormId) -> Option<FormId> {
        self.first_leaf(actor, parent, None, true, &|_| false).map(|(leaf, _)| leaf)
    }

    fn first_leaf(&mut self, actor: FormId, node: FormId, kind: Option<FidgetKind>, gated: bool, held_back: &dyn Fn(FormId) -> bool) -> Option<(FormId, bool)> {
        let data = self.data;
        for &child in data.idle_children.get(&Some(node)).into_iter().flatten() {
            let leaf = !data.idle_children.contains_key(&Some(child));
            if (leaf && (!data.idles[&child].has_file() || held_back(child))) || !self.idle_allowed(actor, child) {
                continue;
            }
            let gated = gated || kind.is_some_and(|k| k.gates(data, child));
            if leaf {
                return Some((child, gated));
            }
            if let Some(found) = self.first_leaf(actor, child, kind, gated, held_back) {
                return Some(found);
            }
        }
        None
    }

    /// The actors in a conversation and their part in it, for the talking gestures
    /// (docs/spec-notes/idles.md §12): the actor the player talks with (saying a line,
    /// or listening while the player chooses), and both sides of a conversation between
    /// actors (the one saying the line, and the one it says it to). The player is left
    /// out.
    pub fn talk_parts(&self) -> Vec<TalkPart> {
        let mut out: Vec<TalkPart> = Vec::new();
        if let Some(c) = self.dialogue.as_ref() {
            let line = c.line.as_ref();
            out.push(TalkPart { actor: c.actor, partner: FormId::PLAYER_REF, talking: line.is_some(), info: line.map(|l| l.info), response: line.map_or(0, |l| l.response) });
        }
        for s in self.speech.iter().filter(|s| s.conversation.is_some()) {
            let Some(to) = s.target else { continue };
            for (actor, partner, talking) in [(s.speaker, to, true), (to, s.speaker, false)] {
                if actor != FormId::PLAYER_REF && !out.iter().any(|p| p.actor == actor) {
                    out.push(TalkPart { actor, partner, talking, info: Some(s.info), response: s.response });
                }
            }
        }
        out
    }

    /// A talking or listening gesture for `actor`, who is in a conversation with
    /// `partner`, from the idle tree, with the nodes gone into on the way (the root
    /// first, the pick last); `None` when the walk finds nothing, or finds a leaf that
    /// is not under a node of the tree's dialogue groups. `folder` and `held_back` as
    /// for [`Game::weapon_fidget`]. The rules (docs/spec-notes/idles.md §12):
    ///
    /// - The walk is the fidgets' [hypothesis: animation.md §2.2; idles.md §9, the
    ///   editor's picking]: roots of the actor's folder in chain order, the first whose
    ///   conditions pass is gone into, and so on down; a node whose children all fail is
    ///   left for its next sibling. The conditions' target is `partner` (the people's
    ///   branch for conversations between actors asks whether the target is the player).
    /// - The pick is kept only when a node on its way is a dialogue node
    ///   ([`talk_node`]) [design: the rest of the idle manager is not built]. The tree
    ///   itself chooses between talking and listening (`IsTalking`), the mood
    ///   (`GetDialogueEmotion`), sitting or standing, power armour, and keeps a pose
    ///   going (`IsLastIdlePlayed`).
    /// - `GetRandomPercent` in these conditions draws from its own generator, so the
    ///   picks do not move the scripts' random numbers [engineering choice].
    pub fn talk_gesture(&mut self, actor: FormId, partner: FormId, folder: &str, held_back: &dyn Fn(FormId) -> bool) -> Option<(FormId, Vec<FormId>)> {
        let data = self.data;
        let folder = folder.to_ascii_lowercase();
        let own = |id: &FormId| data.idles.get(id).and_then(|d| d.model.as_deref()).is_some_and(|m| m.to_ascii_lowercase().starts_with(&folder));
        let roots: Vec<FormId> = data.idle_children.get(&None).into_iter().flatten().copied().filter(|r| own(r) && data.idle_children.contains_key(&Some(*r))).collect();
        std::mem::swap(&mut self.rng, &mut self.idle_rng);
        let mut found = None;
        for root in roots {
            if !self.idle_allowed_with(actor, Some(partner), root) {
                continue;
            }
            let mut way = vec![root];
            if let Some((leaf, talk)) = self.talk_leaf(actor, partner, root, talk_node(data, root), held_back, &mut way) {
                found = talk.then_some((leaf, way));
                break;
            }
        }
        std::mem::swap(&mut self.rng, &mut self.idle_rng);
        found
    }

    /// The first leaf below `node` whose way down passes for `actor` talking with
    /// `partner` (the conditions' target), and whether a node on that way (or `talk`,
    /// said of those above) is a dialogue node ([`talk_node`]); `way` gets the nodes
    /// gone into, the leaf last. The walk of [`Game::weapon_fidget`] otherwise.
    fn talk_leaf(&mut self, actor: FormId, partner: FormId, node: FormId, talk: bool, held_back: &dyn Fn(FormId) -> bool, way: &mut Vec<FormId>) -> Option<(FormId, bool)> {
        let data = self.data;
        for &child in data.idle_children.get(&Some(node)).into_iter().flatten() {
            let leaf = !data.idle_children.contains_key(&Some(child));
            if (leaf && (!data.idles[&child].has_file() || held_back(child))) || !self.idle_allowed_with(actor, Some(partner), child) {
                continue;
            }
            let talk = talk || talk_node(data, child);
            way.push(child);
            if leaf {
                return Some((child, talk));
            }
            if let Some(found) = self.talk_leaf(actor, partner, child, talk, held_back, way) {
                return Some(found);
            }
            way.pop();
        }
        None
    }

    /// The Begin (0), End (1) or Change (2) idle of a package, asked of its actor
    /// ([`Game::package_event`]).
    pub(crate) fn ask_package_idle(&mut self, actor: FormId, package: FormId, which: usize) {
        let Some(def) = self.package_def(&package) else { return };
        let source = [IdleSource::PackageBegin, IdleSource::PackageEnd, IdleSource::PackageChange][which.min(2)];
        if let Some(idle) = def.events[which.min(2)].idle {
            self.ask_idle(actor, idle, source);
        }
    }

    /// The idles of response `response` (index in record order) of line `info`: the
    /// speaker's for `speaker`, the listener's for `listener` [documented: GECK wiki,
    /// Category:Dialogue, each response can name an idle for the speaker and one for the
    /// listener].
    pub(crate) fn ask_response_idles(&mut self, speaker: FormId, listener: Option<FormId>, info: FormId, response: usize) {
        let dv = self.dialogue_view();
        let Some(r) = dv.info(&info).and_then(|d| d.responses.get(response)) else { return };
        let (s, l) = (r.speaker_idle, r.listener_idle);
        if let Some(idle) = s {
            self.ask_idle(speaker, idle, IdleSource::Speaker);
        }
        if let (Some(idle), Some(listener)) = (l, listener) {
            self.ask_idle(listener, idle, IdleSource::Listener);
        }
    }
}

/// One actor's part in a conversation ([`Game::talk_parts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TalkPart {
    pub actor: FormId,
    /// The one it talks with (the player, or the other actor).
    pub partner: FormId,
    /// Saying a line (else listening).
    pub talking: bool,
    /// The line being said (by the actor, or to it), and the position of its response
    /// in the order they are said.
    pub info: Option<FormId>,
    pub response: usize,
}

/// Whether an IDLE is a node of the tree's dialogue groups: one of its conditions asks
/// for the dialogue procedure (`GetCurrentAIProcedure == 4`), the dialogue menu
/// (`MenuMode 1009 == 1`) or the actor saying a line (`IsTalking == 1`) [observed: the
/// people's `DialogueIdles` and its chair, stool, wall-lean and floor-sitting groups,
/// and the robots' and Mr. House's dialogue groups, each have one of these; idles.md
/// §12.1].
pub fn talk_node(data: &crate::data::GameData, idle: FormId) -> bool {
    use crate::cond::{Cmp, Operand};
    let conds = data.idles.get(&idle).and_then(|d| d.conditions.as_deref()).unwrap_or_default();
    conds.iter().any(|c| {
        let is = |v: f64| c.cmp == Cmp::Eq && c.rhs == Operand::Value(v);
        match c.opcode() {
            GET_CURRENT_AI_PROCEDURE => is(4.0),
            IS_TALKING => is(1.0),
            MENU_MODE => is(1.0) && matches!(c.params.first(), Some(ArgValue::Enum(m)) if *m == crate::dialogue::DIALOGUE_MENU),
            _ => false,
        }
    })
}

const GET_CURRENT_AI_PROCEDURE: u16 = 0x108F;

/// GetUnconscious's condition function index [documented: schemas/script/
/// command_opcodes.json, GetUnconscious 4338 = 0x1000 + 242].
const GET_UNCONSCIOUS: u16 = 242;
const IS_TALKING: u16 = 0x108D;
const MENU_MODE: u16 = 0x1024;

/// IsLastIdlePlayed's condition index: it has no page in the script catalogue and no
/// script opcode [documented: xEdit wbCTDAFunctions names index 451 IsLastIdlePlayed,
/// parameter an IDLE; animation.md §1.2]. 86 IDLE conditions use it [observed].
pub const IS_LAST_IDLE_PLAYED: u16 = 451;

/// IsLastIdlePlayed `idle`: 1 when the last idle the actor played is `idle` [hypothesis
/// from the name: the dialogue groups use it to keep a pose going, arms crossed once
/// crossed; idles.md §12.1]. The last idle played is the last one the animating layer
/// reported ([`Game::set_idle_playing`]).
pub(crate) fn is_last_idle_played(g: &Game, on: Option<FormId>, params: &[ArgValue]) -> f64 {
    let want = match params.first() {
        Some(ArgValue::Value(Value::Ref(r))) if *r != 0 => FormId(*r),
        _ => return 0.0,
    };
    let last = on.and_then(|a| g.extra.get(&a)).and_then(|e| e.last_idle);
    if last == Some(want) {
        1.0
    } else {
        0.0
    }
}

/// Whether conditions require `opcode == value`: such a condition not joined to another
/// by OR.
fn requires(conds: &[Condition], opcode: u16, value: f64) -> bool {
    conds.iter().enumerate().any(|(i, c)| {
        let joined = c.or || (i > 0 && conds[i - 1].or);
        c.opcode() == opcode && c.cmp == crate::cond::Cmp::Eq && c.rhs == crate::cond::Operand::Value(value) && !joined
    })
}

/// Whether conditions require `opcode != value`: such a condition not joined to another
/// by OR (`EatIdles`' `IsWeaponOut != 1`).
fn requires_not(conds: &[Condition], opcode: u16, value: f64) -> bool {
    conds.iter().enumerate().any(|(i, c)| {
        let joined = c.or || (i > 0 && conds[i - 1].or);
        c.opcode() == opcode && c.cmp == crate::cond::Cmp::Ne && c.rhs == crate::cond::Operand::Value(value) && !joined
    })
}

/// Whether conditions ask for `opcode == value`, alone or as one of several joined by OR.
fn names(conds: &[Condition], opcode: u16, value: f64) -> bool {
    conds.iter().any(|c| c.opcode() == opcode && c.cmp == crate::cond::Cmp::Eq && c.rhs == crate::cond::Operand::Value(value))
}

/// Opcodes of `IsWeaponOut`, `GetSitting` and `GetSleeping` (`functions::mod`).
const IS_WEAPON_OUT: u16 = 0x1065;
const GET_SITTING: u16 = 0x109F;
const GET_SLEEPING: u16 = 0x1031;
/// Opcode of `GetIsAlerted` (`functions::mod`).
const GET_IS_ALERTED: u16 = 0x105B;
/// Opcode of `GetPlantedExplosive` (`functions::mod`).
const GET_PLANTED_EXPLOSIVE: u16 = 0x11AC;
/// Opcode of `GetUsedItemActivate` (`functions::mod`; condition index 259).
const GET_USED_ITEM_ACTIVATE: u16 = 0x1103;
/// The procedure of a fleeing actor, 16 FLEE (`functions::ai_state`).
const FLEE_PROCEDURE: f64 = 16.0;

/// `actor.PlayIdle name`: the actor plays the idle whose Editor ID is `name`, whatever
/// its conditions say [documented: catalogue notes on PickIdle, "the forced-idle
/// function plays one regardless of conditions"; the parameter is the Editor ID as text,
/// scripts.md §3].
pub fn play_idle(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let idle = match a.get(0) {
        Some(ArgValue::Str(name)) => g.data.by_edid(name).ok_or_else(|| format!("no form named {name}"))?,
        _ => a.form(0)?,
    };
    g.ask_idle(actor, idle, IdleSource::Script);
    none()
}

/// `actor.PickIdle`: the actor chooses an idle from the idle tree by its conditions and
/// plays it; nothing when none fits [documented: GECK wiki PickIdle]. The rules do not
/// know the actor's animation folder (its skeleton), so the call asks the animating
/// layer ([`Game::idle_picks`]), which walks the tree for the actor it draws
/// ([`Game::pick_idle`]) and asks for the pick as any other idle ([`IdleSource::Picked`]);
/// an actor that is not drawn plays nothing, as for every idle. The shipped calls are
/// one object script's OnStartCombat (SCPT 0x0017491E: `PickIdle`, and again when
/// GetCurrentAIProcedure is 21) [observed: decompiled scripts, 2026-10-19].
pub fn pick_idle(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    if !g.idle_picks.contains(&actor) {
        g.idle_picks.push(actor);
    }
    none()
}

/// 1 while the calling actor plays a special idle [documented: catalogue summary]: an
/// idle it was asked to play here and the animating layer reports as playing, or while
/// it sits on (or lies in) furniture, GetSitting 3 [observed: Easy Pete sitting read
/// IsIdlePlaying 1 and GetSitting 3, function probes 2026-10-17; hypothesis for beds].
pub fn is_idle_playing(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let seated = g.furniture_use(actor).is_some_and(|u| u.state == crate::functions::furniture::SEATED);
    yes(g.idle_playing(actor).is_some() || seated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(run_in_sequence: bool, do_once: bool, timer: f32, n: u32) -> IdleCollection {
        IdleCollection { run_in_sequence, do_once, timer, idles: (1..=n).map(FormId).collect() }
    }

    fn take(p: &mut IdlePicker, c: &IdleCollection, n: usize) -> Vec<Option<(u32, f32)>> {
        (0..n).map(|_| p.next(c, |_| 1).map(|(f, w)| (f.0, w))).collect()
    }

    #[test]
    fn a_sequence_runs_from_the_top_and_waits_between_rounds() {
        let c = collection(true, false, 4.0, 3);
        let got = take(&mut IdlePicker::default(), &c, 7);
        let want = [(1, 0.0), (2, 0.0), (3, 0.0), (1, 4.0), (2, 0.0), (3, 0.0), (1, 4.0)];
        assert_eq!(got, want.map(Some));
    }

    #[test]
    fn do_once_plays_the_sequence_once() {
        let c = collection(true, true, 4.0, 2);
        assert_eq!(take(&mut IdlePicker::default(), &c, 4), [Some((1, 0.0)), Some((2, 0.0)), None, None]);
        // A random collection flagged Do Once: one idle.
        let c = collection(false, true, 4.0, 3);
        assert_eq!(take(&mut IdlePicker::default(), &c, 2), [Some((2, 0.0)), None]);
    }

    #[test]
    fn random_picks_wait_the_timer_between_idles() {
        let c = collection(false, false, 2.5, 3);
        let mut p = IdlePicker::default();
        let mut rolls = [2usize, 0, 9].into_iter();
        let got: Vec<(u32, f32)> = (0..3).map(|_| p.next(&c, |_| rolls.next().unwrap()).map(|(f, w)| (f.0, w)).unwrap()).collect();
        // A roll outside the list is held to its last entry.
        assert_eq!(got, [(3, 0.0), (1, 2.5), (3, 2.5)]);
        assert_eq!(IdlePicker::default().next(&collection(false, false, 1.0, 0), |_| 0), None);
    }

    #[test]
    fn loop_counts() {
        let d = |loop_min, loop_max| IdleDef { model: None, loop_min, loop_max, conditions: None, ..Default::default() };
        assert_eq!(d(0, 0).loops(|_| 0), Loops::Times(1));
        assert_eq!(d(255, 255).loops(|_| 0), Loops::Forever);
        // Loop Forever is both at 255; a range up to 255 is a range.
        assert_eq!(d(0, 255).loops(|_| 0), Loops::Times(1));
        assert_eq!(d(3, 255).loops(|n| n - 1), Loops::Times(255));
        assert_eq!(d(2, 5).loops(|n| n - 1), Loops::Times(5));
        assert_eq!(d(2, 5).loops(|_| 0), Loops::Times(2));
        // A minimum above the maximum counts as the maximum.
        assert_eq!(d(4, 3).loops(|_| 0), Loops::Times(3));
    }

    /// An IDLE's MODL and DATA as the schema decodes them (`gameplay.json` IDLE).
    fn idle(modl: Option<&[u8]>, data: Option<&[u8]>) -> IdleDef {
        let schema = fnv_esm::Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        let mut ctx = fnv_esm::decode::DecodeCtx { schema: &schema, on_formid: None };
        let mut val = |sig: &[u8; 4], b: Option<&[u8]>| b.and_then(|b| fnv_esm::decode::decode_sub(b"IDLE", &fnv_esm::Subrecord { sig: *sig, data: b.to_vec(), extended: false }, &mut ctx).value);
        let model = val(b"MODL", modl);
        let data = val(b"DATA", data);
        IdleDef::from_fields(model.as_ref().and_then(|m| m.as_str()), data.as_ref())
    }

    #[test]
    fn idle_records_read_through_the_schema() {
        let d = idle(Some(b"Characters\\_Male\\IdleAnims\\Talk.KF\0"), Some(&[0x47, 2, 3, 0, 0, 0, 0, 0]));
        assert_eq!((d.model.as_deref(), d.loop_min, d.loop_max, d.has_file()), (Some("Characters\\_Male\\IdleAnims\\Talk.KF"), 2, 3, true));
        // An inner node of the tree names a folder; the short DATA form has six bytes.
        let d = idle(Some(b"Characters\\_Male\\IdleAnims\0"), Some(&[0, 255, 255, 0, 0, 0]));
        assert_eq!((d.has_file(), d.loop_min, d.loop_max), (false, 255, 255));
        assert!(!idle(None, None).has_file());
        // The replay delay is the i16 after the unused byte, signed.
        assert_eq!(idle(None, Some(&[0, 0, 0, 0, 45, 0])).replay_delay, 45);
        assert_eq!(idle(None, Some(&[0, 0, 0, 0, 0xFF, 0xFF])).replay_delay, -1);
        // "No attacking" is bit 0 of the flags byte after it (`NVgeckoHiss`); the short
        // form has none.
        assert!(idle(None, Some(&[7, 1, 1, 0, 0, 0, 1, 0])).no_attacking);
        assert!(!idle(None, Some(&[7, 1, 1, 0, 0, 0])).no_attacking);
        // The section is the group byte's low six bits: 0x47 Special Idle (loose),
        // 0x55 Upper Body.
        assert_eq!(idle(None, Some(&[0x47, 1, 2, 0, 0, 0])).section, 7);
        assert_eq!(idle(None, Some(&[0x55, 1, 2, 0, 0, 0])).section, crate::data::IDLE_SECTION_UPPER_BODY);
    }

    #[test]
    fn fidget_kinds_are_gated_by_a_required_condition() {
        use crate::cond::{Cmp, Operand, RunOn};
        let c = |function: u16, cmp: Cmp, v: f64, or: bool| Condition { cmp, or, run_on: RunOn::Subject, function, params: Vec::new(), rhs: Operand::Value(v) };
        let (sitting, sleeping, weapon, random) = (GET_SITTING - 0x1000, GET_SLEEPING - 0x1000, IS_WEAPON_OUT - 0x1000, 0x4D);
        let on = FidgetKind::OnFurniture;
        // `SittingChairIdles`: GetSitting == 3 among others.
        assert!(on.gated_by(&[c(random, Cmp::Lt, 50.0, false), c(sitting, Cmp::Eq, 3.0, false)]));
        // `BedSpecialIdles`: GetSleeping == 3.
        assert!(on.gated_by(&[c(sleeping, Cmp::Eq, 3.0, false)]));
        // The root (> 0, joined by OR), getting on (== 2), and an OR-joined 3: no gate.
        assert!(!on.gated_by(&[c(sitting, Cmp::Gt, 0.0, true), c(sleeping, Cmp::Gt, 0.0, false)]));
        assert!(!on.gated_by(&[c(sitting, Cmp::Eq, 2.0, false)]));
        assert!(!on.gated_by(&[c(sitting, Cmp::Eq, 3.0, true), c(random, Cmp::Lt, 5.0, false)]));
        // The two kinds do not gate each other's branches.
        assert!(FidgetKind::WeaponOut.gated_by(&[c(weapon, Cmp::Eq, 1.0, false)]));
        assert!(!on.gated_by(&[c(weapon, Cmp::Eq, 1.0, false)]));
        assert!(!FidgetKind::WeaponOut.gated_by(&[c(sitting, Cmp::Eq, 3.0, false)]));
        assert_eq!(on.source(), IdleSource::FurnitureFidget);
    }

    #[test]
    fn holstered_fidgets_are_gated_by_the_weapon_away_alone() {
        use crate::cond::{Cmp, Operand, RunOn};
        let c = |function: u16, cmp: Cmp, v: f64, or: bool| Condition { cmp, or, run_on: RunOn::Subject, function, params: Vec::new(), rhs: Operand::Value(v) };
        let (weapon, procedure, random) = (IS_WEAPON_OUT - 0x1000, 0x8F, 0x4D);
        let away = FidgetKind::WeaponAway;
        // A node wanting IsWeaponOut == 0 among others, and one wanting IsWeaponOut != 1
        // with the eat procedure: both gate.
        assert!(away.gated_by(&[c(0xB6, Cmp::Ne, 1.0, false), c(weapon, Cmp::Eq, 0.0, false)]));
        assert!(away.gated_by(&[c(weapon, Cmp::Ne, 1.0, false), c(procedure, Cmp::Eq, 10.0, false)]));
        // Joined by OR to another, or wanting the weapon out: no gate.
        assert!(!away.gated_by(&[c(weapon, Cmp::Eq, 0.0, true), c(random, Cmp::Lt, 5.0, false)]));
        assert!(!away.gated_by(&[c(random, Cmp::Lt, 5.0, true), c(weapon, Cmp::Ne, 1.0, false)]));
        assert!(!away.gated_by(&[c(weapon, Cmp::Eq, 1.0, false)]));
        assert!(!away.gated_by(&[c(weapon, Cmp::Ne, 0.0, false)]));
        // The weapon-out kind does not take the weapon-away branches.
        assert!(!FidgetKind::WeaponOut.gated_by(&[c(weapon, Cmp::Eq, 0.0, false)]));
        assert_eq!(away.source(), IdleSource::HolsteredFidget);
    }

    #[test]
    fn creature_combat_idles_are_gated_by_get_is_alerted_alone_or_or_joined() {
        use crate::cond::{Cmp, Operand, RunOn};
        let c = |function: u16, cmp: Cmp, v: f64, or: bool| Condition { cmp, or, run_on: RunOn::Subject, function, params: Vec::new(), rhs: Operand::Value(v) };
        let (moving, alerted, procedure, in_combat) = (0x19, GET_IS_ALERTED - 0x1000, 0x8F, 0x121);
        let a = FidgetKind::Alerted;
        // NVGeckoCombatIdles 0x0011D9E9: IsMoving != 1 AND (GetIsAlerted == 1 OR
        // GetCurrentAIProcedure == 13 OR == 16) [observed: FalloutNV.esm].
        assert!(a.gated_by(&[c(moving, Cmp::Ne, 1.0, false), c(alerted, Cmp::Eq, 1.0, true), c(procedure, Cmp::Eq, 13.0, true), c(procedure, Cmp::Eq, 16.0, false)]));
        // MWarning 0x00072DC2: GetIsAlerted == 1 alone.
        assert!(a.gated_by(&[c(alerted, Cmp::Eq, 1.0, false)]));
        // NVGeckoGeneralIdles (IsInCombat == 0) is no combat idle; nor is the weapon branch.
        assert!(!a.gated_by(&[c(in_combat, Cmp::Eq, 0.0, false)]));
        assert!(!a.gated_by(&[c(IS_WEAPON_OUT - 0x1000, Cmp::Eq, 1.0, false)]));
        assert!(!FidgetKind::WeaponOut.gated_by(&[c(alerted, Cmp::Eq, 1.0, false)]));
        assert_eq!(a.source(), IdleSource::CombatIdle);
    }

    #[test]
    fn a_calm_creature_walks_the_whole_tree_by_its_own_conditions() {
        use crate::cond::{Cmp, Operand, RunOn};
        let c = |function: u16, cmp: Cmp, v: f64| Condition { cmp, or: false, run_on: RunOn::Subject, function, params: Vec::new(), rhs: Operand::Value(v) };
        let calm = FidgetKind::Calm;
        // The protectron's CommonIdle (IsInCombat == 0) and its leaves (GetRandomPercent,
        // IsMoving) are no gate any kind names: every branch is a calm creature's, and
        // the conditions met on the walk decide [observed: #1977 probe 2].
        assert!(calm.gated_by(&[c(0x121, Cmp::Eq, 0.0)]));
        assert!(calm.gated_by(&[]));
        assert!(!FidgetKind::Alerted.gated_by(&[c(0x121, Cmp::Eq, 0.0)]));
        assert_eq!(calm.source(), IdleSource::CalmIdle);
    }

    #[test]
    fn the_tree_orders_siblings_by_their_previous_links() {
        let node = |parent: Option<u32>, previous: Option<u32>| IdleDef { parent: parent.map(FormId), previous: previous.map(FormId), ..Default::default() };
        // Root 1 with children 4 → 2 → 3 (chained), and 9 outside the chain; root 10 after 1.
        let idles: std::collections::HashMap<FormId, IdleDef> = [
            (1, node(None, None)),
            (10, node(None, Some(1))),
            (2, node(Some(1), Some(4))),
            (3, node(Some(1), Some(2))),
            (4, node(Some(1), None)),
            (9, node(Some(1), Some(77))),
        ]
        .into_iter()
        .map(|(id, d)| (FormId(id), d))
        .collect();
        let t = crate::data::idle_children(&idles);
        assert_eq!(t[&None], vec![FormId(1), FormId(10)]);
        // 9's previous is not a sibling: it starts a chain of its own, after 4's.
        assert_eq!(t[&Some(FormId(1))], vec![FormId(4), FormId(2), FormId(3), FormId(9)]);
        assert!(!t.contains_key(&Some(FormId(2))));
    }
}
