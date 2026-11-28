//! The rest of the xNVSE "other" group and the player-control locks
//! (docs/handoff/xnvse-remaining.md §3) [documented: their pages in the wiki export].

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeMap, HashMap, HashSet};

/// xNVSE's outer list contains one path array per current objective target; each
/// path ends with the target (GECK wiki GetCurrentQuestObjectiveTeleportLinks).
/// SUP's Alt interface instead returns the first visible marker of each path.
pub(crate) fn objective_teleport_links(g: &mut Game, flat: bool) -> XVal {
    use crate::quest_targets::{DoorGraph, Place};
    let mut paths = Vec::new();
    if let Some(quest) = g.ui.active_quest {
        let current = g.quest_state(quest).objectives.iter()
            .find(|(_, o)| o.displayed && !o.completed).map(|(&i, _)| i);
        let here = g.cell_of(FormId::PLAYER_REF).map(|c| Place::of_cell(g.world.placed, c));
        if let (Some(current), Some(here)) = (current, here) {
            let targets = g.check_quest_targets(quest);
            g.other.objective_doors.get_or_insert_with(|| DoorGraph::build(g.world.placed));
            let graph = g.other.objective_doors.as_ref().expect("initialized above");
            for target in targets.into_iter().filter(|t| t.objective == current && t.shown) {
                let Some(there) = g.cell_of(target.target).map(|c| Place::of_cell(g.world.placed, c)) else { continue; };
                let Some(mut path) = graph.path(&g.world, here, there, target.ignores_locks) else { continue; };
                path.push(target.target);
                paths.push(path);
            }
        }
    }
    let mut entries = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        let value = if flat {
            XVal::Ref(path[0].0)
        } else {
            let row = path.into_iter().enumerate().map(|(i, r)| (XKey::Num(i as f64), XVal::Ref(r.0))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, row))
        };
        entries.push((XKey::Num(index as f64), value));
    }
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// What these functions keep for the session (none of it is saved: DisablePlayerControlsAlt
/// "changes are NOT savebaked" [documented]; the rest [hypothesis]).
#[derive(Debug, Default)]
pub struct OtherState {
    /// Bounded SetMaxAniso request; last call wins until the renderer consumes it. Not saved.
    pub(crate) texture_filter_level: Option<u16>,
    /// The immutable placed-door graph, built lazily; run-time lock state is read
    /// for every route. Not persisted or rebuilt on each minimap callback.
    pub objective_doors: Option<crate::quest_targets::DoorGraph>,
    /// The plugin and the script whose code made the extender call now running
    /// (`Exec::ext_call`); "" and `None` for the console, a query or a runtime script.
    pub plugin: String,
    pub script: Option<FormId>,
    /// DisablePlayerControlsAlt's locks, by the plugin that set them [documented: "stored
    /// on a per-mod basis"], in the order of `Player::controls_disabled`.
    pub alt_locks: BTreeMap<String, [bool; 7]>,
    /// SetEditorID's names.
    pub edids: HashMap<FormId, String>,
    /// SetNthDefaultForm's changes, by place in the DOBJ list.
    pub defaults: HashMap<usize, FormId>,
    /// SetDoorTeleport's destinations: (door, position, z rotation in radians).
    pub doors: HashMap<FormId, (FormId, [f32; 3], f32)>,
    /// RemoveScript's and SetScript's changes: a base form's script, `None` when taken
    /// away [observed: nvse-fifth, Doc's reference has no script after RemoveScript].
    pub scripts: HashMap<FormId, Option<FormId>>,
    /// The hotkeys 1–8 (SetHotkeyItem).
    pub hotkeys: [Option<FormId>; 8],
    /// Keys SetIsControl made custom controls.
    pub custom_controls: HashSet<u16>,
    /// SetNthFactionRankNameEx's titles: (faction, rank, female).
    pub ranks: HashMap<(FormId, usize, bool), String>,
    /// SetIsPowerArmor's changes.
    pub power_armor: HashMap<FormId, bool>,
    /// SetConsoleEcho's state; off at the start [observed: nvse-fifth, GetConsoleEcho 0].
    pub echo: bool,
    /// SetPlayerCurrentAmmoRounds, for the engine's combat to take.
    pub set_magazine: Option<u32>,
    /// One-shot `EndVATScam` exit request for the engine (`Game::take_end_vats_cam_request`).
    pub end_vats_cam_requested: bool,
    /// AddPackageAt's, RemovePackageAt's and RemoveAllPackages' package lists, by actor
    /// base form (`Game::package_list`).
    pub package_lists: HashMap<FormId, Vec<FormId>>,
    /// SetFlagsLow/High's record flags.
    pub flags: HashMap<FormId, u32>,
    /// Explicit `SetPersistent` session overrides (separate from `flags` so unrelated
    /// `SetReferenceFlag` / `SetFlagsLow` writes do not clear persistent-group refs whose
    /// static record lacks bit `0x400`) [design].
    pub persistent_overrides: HashMap<FormId, bool>,
    /// SetActorBaseFlagsLow/High's 32-bit ACBS flags, by actor base form.
    pub acbs_flags: HashMap<FormId, u32>,
    /// Con_SCOF's file name (nothing is written).
    pub scof: Option<String>,
    /// AddAnimation's and DelAnimation's lists, by actor base.
    pub animations: HashMap<FormId, Vec<String>>,
    /// SetLeveledListFlags' raw flags of item lists (the list keeps three bits).
    pub lev_flags: HashMap<FormId, u8>,
    /// Edited actor lists (LVLN, LVLC) (`Game::lev_list`).
    pub lev_actors: HashMap<FormId, crate::nvse_leveled::LevList>,
    /// Generic*Form's changed lists, by (list code, form) (`Game::generic_list`).
    pub generic: HashMap<(i64, FormId), Vec<FormId>>,
    /// SetOwningFactionRequiredRank's ranks, by reference (over the record's XRNK), read
    /// through `Game::owner_rank` / `Game::ownership_in` and carried across same-run loads
    /// in `Game::Session`.
    pub owner_ranks: HashMap<FormId, i32>,
    /// SetZoneOwner's and session SetZoneFlag bit-1 changes, by encounter zone (`nvse_anh`).
    /// Carried across same-run loads in `Game::Session`.
    pub zone_changes: HashMap<FormId, crate::nvse_anh::ZoneChange>,
    /// SetShowOffDebugMode's switch (nothing more is printed).
    pub showoff_debug: bool,
    /// LandTextureAddGrass's and LandTextureRemoveGrass's grass lists (`nvse_jip5`).
    pub ltex_grasses: HashMap<FormId, Vec<FormId>>,
    /// Lutana batch 1's record changes (`nvse_lutana`).
    pub lutana: crate::nvse_lutana::LutanaState,
    /// SetCasinoChip's chips, by casino (`nvse_jg3`, read through `crate::casino::chip`).
    pub casino_chips: HashMap<FormId, FormId>,
    /// The Set…TerminalMenuItem… changes, by terminal and item (`nvse_jg3`, read through
    /// `crate::terminal::item`).
    pub terminal_items: HashMap<(FormId, usize), crate::terminal::ItemEdit>,
    /// The idle markers' and regions' weather blocks a script changed, whole copies of
    /// the record's (`nvse_jg4`, read through `nvse_jg4::idle_marker` and
    /// `nvse_jg4::region_weathers`).
    pub idle_markers: HashMap<FormId, crate::package::IdleCollection>,
    /// SetIdleMarkerTraitNumeric's flags, the whole byte as set (the collection keeps
    /// only the two bits it plays by; `nvse_jg4`).
    pub idle_marker_flags: HashMap<FormId, u8>,
    pub region_weathers: HashMap<FormId, crate::data::RegionWeathers>,
    /// ITR NVSE's state (`nvse_itr1`).
    pub itr: crate::nvse_itr1::ItrState,
    /// ITR NVSE's batch-2 state (`nvse_itr2`).
    pub itr2: crate::nvse_itr2::Itr2State,
    /// JIP batch 6's changes: destruction data, creature models, messages (`nvse_jip6`).
    pub jip6: crate::nvse_jip6::Jip6State,
    /// JIP batch 7's lists and settings: ammo effects, climate weathers, armour (`nvse_jip7`).
    pub jip7: crate::nvse_jip7::Jip7State,
    /// JIP batch 8's changes: actor effects, base effects, water effects, excluded combat
    /// actions (`nvse_jip8`).
    pub jip8: crate::nvse_jip8::Jip8State,
    /// JIP batch 9's switches and what the engine tells: the HUD cursor, volumes, fonts,
    /// colours, immortal mode, disabled interaction types (`nvse_jip9`).
    pub jip9: crate::nvse_jip9::Jip9State,
    /// JIP batch 10's objectives, added variables, disabled scripts, extra data, map
    /// markers (`nvse_jip10`).
    pub jip10: crate::nvse_jip10::Jip10State,
    /// JIP batch 14's effects, primitives, models, VATS, fuses and frequencies (`nvse_jip14`).
    pub jip14: crate::nvse_jip14::Jip14State,
    /// The unlisted JIP commands' locks, switches and lights (`nvse_unlisted`).
    pub unlisted: crate::nvse_unlisted::UnlistedState,
    /// The New Game flag of the unlisted commands' batch 1 (`nvse_unlisted2`).
    pub unlisted2: crate::nvse_unlisted2::Unlisted2State,
    /// SUP's cached JSON files (`nvse_json`).
    pub json: crate::nvse_json::JsonState,
    /// JIP batch 11's actor overrides and engine requests (`nvse_jip11`).
    pub jip11: crate::nvse_jip11::Jip11State,
    /// JIP batch 13's physics view, requests and follows (`nvse_jip13`).
    pub jip13: crate::nvse_jip13::Jip13State,
    /// JIP batch 12's sky, world, barter and menu state (`nvse_jip12`).
    pub jip12: crate::nvse_jip12::Jip12State,
    /// JohnnyGuitar batch 5's record changes (`nvse_jg5`).
    pub jg5: crate::nvse_jg5::Jg5State,
    /// JohnnyGuitar batch 6's state (`nvse_jg6`).
    pub jg6: crate::nvse_jg6::Jg6State,
    /// JohnnyGuitar batch 7's switches, settings and queued texts (`nvse_jg7`).
    pub jg7: crate::nvse_jg7::Jg7State,
    /// JohnnyGuitar batch 8's handlers, custom map marker and radio tuning (`nvse_jg8`).
    pub jg8: crate::nvse_jg8::Jg8State,
    /// JohnnyGuitar batch 9's sounds from files and scripted holotapes (`nvse_jg9`).
    pub jg9: crate::nvse_jg9::Jg9State,
    /// JohnnyGuitar batch 10's requests and switches for the engine (`nvse_jg10`).
    pub jg10: crate::nvse_jg10::Jg10State,
}

/// The bit of DisablePlayerControlsAltEx's flags for each control group, in the order of
/// `Player::controls_disabled` (movement, Pip-Boy, fighting, POV, looking, rollover,
/// sneaking). Movement 0, Pip-Boy 2, fighting 3, POV 4 [documented: DisablePlayerControlsAltEx,
/// the default `(1 << 0) | (1 << 2) | (1 << 3) | (1 << 4)` and "the Fighting flag is the
/// fourth bit"]; looking 1, rollover 5, sneaking 6, bit 7 nothing [observed: nvse-fifth,
/// each bit read back through GetPlayerControlsDisabledAlt].
pub const ALT_BITS: [u32; 7] = [0, 2, 3, 4, 1, 5, 6];

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// The groups a DisablePlayerControlsAlt-family call names: the seven flags in order, a
/// left-out one taking its place in `dflt`, or the Ex form's bitmask; `dflt` when there is
/// no argument [observed: nvse-fifth, `DisablePlayerControlsAlt 1` also locked the
/// Pip-Boy; hypothesis: the left-out flags are the no-argument default's]. The getters
/// read only the flags they name (`short` all false) [hypothesis].
fn alt_groups(ex: bool, args: &[XVal], dflt: [bool; 7], short: [bool; 7]) -> [bool; 7] {
    if args.is_empty() {
        return dflt;
    }
    if ex {
        let bits = num(args, 0, 0.0) as u32;
        return ALT_BITS.map(|b| bits >> b & 1 != 0);
    }
    std::array::from_fn(|i| if i < args.len() { num(args, i, 0.0) != 0.0 } else { short[i] })
}

impl<'a> Game<'a> {
    /// Change a package for the session: a copy of its definition, changed, takes its
    /// place for every reader (`Game::package_def`). A change that leaves the definition
    /// as it is makes no copy: the copies live for the session (readers hold `&'a`), and a
    /// mod's main-loop script may call a setter with the same values every frame (#3150).
    pub(crate) fn change_package(&mut self, p: FormId, change: impl FnOnce(&mut crate::package::PackageDef)) {
        let Some(def) = self.package_def(&p) else { return };
        let mut new = def.clone();
        change(&mut new);
        if new == *def {
            return;
        }
        let leaked: &'a crate::package::PackageDef = Box::leak(Box::new(new));
        self.package_overrides.insert(p, leaked);
    }

    /// A form's record flags, SetFlagsLow/High's change first, with the bits the game
    /// sets at run time [observed: nvse-seventh and nvse-flags]:
    /// - a form from a master file has bits 0 and 3 (ESM, "form cannot be saved"): the
    ///   10mm pistol 9 though its record has none;
    /// - an actor that has talked to the player has bit 6 (0x40): Doc and Ringo 1097,
    ///   Chet, Sunny and Easy Pete 1033, none with 0x40 in its record (a REFR keeps its
    ///   record's 0x40); hypothesis: the game keeps the same state as GetTalkedToPC;
    /// - the player reads 0x408;
    /// - a reference made at run time reads 0x108 (PlaceAtMe'd creature 264), with 0x400
    ///   for an NPC (1288).
    fn record_flags(&self, f: FormId) -> u32 {
        if let Some(&v) = self.other.flags.get(&f) {
            return v;
        }
        if f == FormId::PLAYER_REF {
            return 0x408;
        }
        if self.world.placed.get(f).is_none() && self.world.peek(f).is_some() {
            let npc = self.world.peek(f).is_some_and(|s| self.data.sig(s.base) == Some(*b"NPC_"));
            return 0x108 | if npc { 0x400 } else { 0 };
        }
        let f = self.unclone(f);
        let own = self.data.form_flags.get(&f).copied().unwrap_or(0);
        let master = self.data.load_order.get((f.0 >> 24) as usize).is_some_and(|p| p.to_ascii_lowercase().ends_with(".esm"));
        let talked = self.extra.get(&f).is_some_and(|e| e.talked_to_pc);
        own | if master { 0x9 } else { 0 } | if talked { 0x40 } else { 0 }
    }

    /// A base form's script, RemoveScript's and SetScript's changes first.
    pub fn base_script(&self, base: FormId) -> Option<FormId> {
        match self.other.scripts.get(&base) {
            Some(s) => *s,
            None => self.data.script_of_base(base),
        }
    }
}

/// The start of the Alt lock keys NVForever's own mini-games in the world hold the
/// player's controls under (the drinking contest's `nvforever:drink_contest`): no
/// plugin's name starts so [design]. Such a lock is not a story scene
/// ([`Game::story_locks`]); it holds the corner and quest notices while it lasts (nvf-play
/// `corner_messages::in_world_game`).
pub const ENGINE_GAME_LOCK_PREFIX: &str = "nvforever:";

impl Game<'_> {
    /// The groups held by a story script's DisablePlayerControls ([`Game::story_locked`])
    /// and any plugin's Alt lock: what a co-op host's scene holds its guests with [design:
    /// only story scenes]. A lock from the script of something the host
    /// uses (a companion's repair message boxes, an activator, a terminal) is the host's
    /// alone.
    /// NVForever's own in-world mini-games' locks ([`ENGINE_GAME_LOCK_PREFIX`], the
    /// drinking contest's) are each game's own, not a scene: every player at the table
    /// holds its own controls, so a guest is not put on the host's spot.
    pub fn story_locks(&self) -> [bool; 7] {
        let mut out = [false; 7];
        for ((o, d), s) in out.iter_mut().zip(self.player.controls_disabled).zip(self.story_locked) {
            *o = d && s;
        }
        for (_, locks) in self.other.alt_locks.iter().filter(|(k, _)| !k.starts_with(ENGINE_GAME_LOCK_PREFIX)) {
            for (o, l) in out.iter_mut().zip(locks) {
                *o |= l;
            }
        }
        out
    }

    /// Whether the script calling with `ctx` tells a story: a quest's script or stage
    /// result, a dialogue result (each runs for a quest), or the script of a trigger
    /// volume (a placed reference whose script has a trigger block: the level designers'
    /// story triggers). An object script on anything else (an activator, a companion, a
    /// terminal) is the player's own business there.
    pub fn is_story_caller(&self, ctx: &crate::functions::Ctx) -> bool {
        if ctx.quest.is_some() {
            return true;
        }
        let Some(r) = ctx.self_ref else { return false };
        let Some(base) = self.world.placed.get(r).map(|p| p.base) else { return false };
        let Some(unit) = self.data.script_of_base(base).and_then(|s| self.data.script(s)) else { return false };
        [crate::game::event::ON_TRIGGER_ENTER, crate::game::event::ON_TRIGGER, crate::game::event::ON_TRIGGER_LEAVE].iter().any(|&b| unit.has_block(b))
    }

    /// [`Game::controls_locked`] without NVForever's own in-world mini-games' locks
    /// ([`ENGINE_GAME_LOCK_PREFIX`]): what a co-op scene holds this player with. At a
    /// drinking contest's table the other drinkers' bodies stay drawn.
    pub fn scene_locks(&self) -> [bool; 7] {
        let mut out = self.player.controls_disabled;
        for (o, l) in out.iter_mut().zip(self.coop_host_locks) {
            *o |= l;
        }
        for (_, locks) in self.other.alt_locks.iter().filter(|(k, _)| !k.starts_with(ENGINE_GAME_LOCK_PREFIX)) {
            for (o, l) in out.iter_mut().zip(locks) {
                *o |= l;
            }
        }
        out
    }

    /// The control groups locked now: DisablePlayerControls', every plugin's
    /// DisablePlayerControlsAlt's and, on a co-op guest, the host's scene's
    /// ([`Game::coop_host_locks`]) (the engine reads this).
    pub fn controls_locked(&self) -> [bool; 7] {
        let mut out = self.player.controls_disabled;
        for (o, l) in out.iter_mut().zip(self.coop_host_locks) {
            *o |= l;
        }
        for locks in self.other.alt_locks.values() {
            for (o, l) in out.iter_mut().zip(locks) {
                *o |= l;
            }
        }
        out
    }

    /// The DOBJ list with SetNthDefaultForm's changes.
    fn default_forms(&self) -> Vec<Option<FormId>> {
        let mut v = self.data.default_objects.clone();
        for (&i, &f) in &self.other.defaults {
            if v.len() <= i {
                v.resize(i + 1, None);
            }
            v[i] = Some(f);
        }
        v
    }

    /// Where a load door leads: SetDoorTeleport's, else the record's (XTEL).
    pub(crate) fn door_teleport(&self, door: FormId) -> Option<(FormId, [f32; 3], f32)> {
        if let Some(&t) = self.other.doors.get(&door) {
            return Some(t);
        }
        let t = self.world.placed.get(door)?.teleport?;
        Some((t.door, t.pos, t.rot[2]))
    }
}

/// A form's record flags as GetFlagsLow sees them (GetReferenceFlag).
pub(crate) fn record_flags_of(g: &Game, f: FormId) -> u32 {
    g.record_flags(f)
}

/// Whether a block's code calls the opcode anywhere, in statements or expressions (the
/// decoded program, searched as data).
fn calls_opcode(v: &serde_json::Value, op: u64) -> bool {
    match v {
        serde_json::Value::Object(m) => m.get("opcode").and_then(serde_json::Value::as_u64) == Some(op) || m.values().any(|x| calls_opcode(x, op)),
        serde_json::Value::Array(a) => a.iter().any(|x| calls_opcode(x, op)),
        _ => false,
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let zero = XVal::Num(0.0);
    let v = match name {
        // The receiver is the target; player perception and self-pairs do not count.
        // Read the existing measured cache, without another sensing pass or expiry rule
        // (actor-behaviour.md §2.4). Missing/nonactor zero is a selected portable
        // convention matching neighboring queries; native invalid behavior is unknown.
        "isactordetected" => {
            let target = subject.filter(|&r| g.def_following(r, |_| false).is_some()
                || matches!(g.data.sig(base(g, r)), Some(sig) if sig == *b"NPC_" || sig == *b"CREA"));
            yes(target.is_some_and(|target| target != FormId::PLAYER_REF
                && g.detection.pairs.iter().any(|(&(detector, seen), (d, _))|
                    seen == target && detector != target && detector != FormId::PLAYER_REF
                        && d.band == crate::detection::Band::Detected)))
        }
        // Four-character record signature, separate from GetObjectType's numeric
        // result (docs/spec-notes/form-type-source-call.md). Strictly ASCII
        // case-sensitive and requires an actual reference (placed, spawned, player,
        // or temporary inventory ref), rejecting direct base forms with 0 [observed:
        // w4g_01_getisformtype_case].
        "getisformtype" => {
            let wanted = match args.first() { Some(XVal::Str(s)) => s.as_bytes(), _ => &[] };
            let actual = subject
                .and_then(|reference| crate::functions::base_of(g, reference).filter(|&b| b != reference))
                .and_then(|base_form| g.data.sig(base_form));
            yes(wanted.len() == 4 && actual.is_some_and(|signature| signature.as_slice() == wanted))
        }
        // [documented: AddItemOwnership; AddItemHealthPercentOwner] `item count [health
        // 0–1] owner rank`: the items come owned (the rank is not kept [hypothesis]), at
        // full health, not AddItem's 99.9% [observed: nvse-fifth, pistol 80 of 80, owner
        // Doc; the health a fraction, 0.5 gives 40].
        "additemownership" | "additemhealthpercentowner" => {
            let (Some(to), Some(item)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let n = num(args, 1, 1.0).max(1.0) as i32;
            let health = name == "additemhealthpercentowner";
            let owner = form(args, if health { 3 } else { 2 });
            let condition = health.then(|| num(args, 2, 1.0).clamp(0.0, 1.0) as f32).filter(|&c| c < 1.0);
            match g.world.inventory_mut(to) {
                Ok(inv) => {
                    inv.add_extra(item, n, fnv_sim::inventory::Extra { condition, owner, ..Default::default() });
                }
                Err(e) => return Some(Err(e.to_string())),
            }
            zero
        }
        // [documented: AddSpellNS] AddSpell without its message.
        "addspellns" => {
            let (Some(a), Some(s)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            if let Err(e) = crate::functions::console::call_words(g, Some(a), "AddSpell", &[format!("{:08X}", s.0)], "AddSpellNS") {
                return Some(Err(e));
            }
            zero
        }
        // [documented: EndVATScam] exits VATS mode and queues a one-shot camera/mode exit
        // request for the engine (`Game::end_vats_cam`, `Game::take_end_vats_cam_request`).
        "endvatscam" => {
            g.end_vats_cam();
            zero
        }
        // [documented: ForEachInList] the UDF runs once per form of the list, in order.
        "foreachinlist" => {
            let (Some(l), Some(f)) = (form(args, 0), args.get(1).cloned()) else { return Some(Ok(zero)) };
            for m in crate::functions::list_members(g, l) {
                if let Err(error) = g.run_function(&f, None, vec![XVal::Ref(m.0)], None, *ctx) {
                    g.emit(crate::trace::EventKind::ScriptError { unit: "ForEachInList".into(), error });
                }
            }
            zero
        }
        // [documented: GetBaseSpellListLevSpells] the base's leveled spells; "leveled
        // spells may be unused in vanilla" and NVForever reads none [hypothesis]; a list
        // GenericAddForm (code 7) made is kept (`crate::nvse_lists`).
        "getbasespelllistlevspells" => {
            let b = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            let items = g.other.generic.get(&(7, b)).into_iter().flatten().enumerate().map(|(i, f)| (XKey::Num(i as f64), XVal::Ref(f.0))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, items))
        }
        // [documented: GetConsoleEcho; SetConsoleEcho, which gives the state before]
        // [hypothesis: echo is on at the start].
        "getconsoleecho" => yes(g.other.echo),
        "setconsoleecho" => {
            let before = g.other.echo;
            g.other.echo = num(args, 0, 1.0) != 0.0;
            yes(before)
        }
        // [documented: GetConsoleOutputFilename; HasConsoleOutputFileName] no SCOF file.
        "getconsoleoutputfilename" => XVal::Str(g.other.scof.clone().unwrap_or_default()),
        "hasconsoleoutputfilename" => yes(g.other.scof.is_some()),
        // [documented: GetCrosshairRef, the reference the player would activate;
        // GetCrosshairRefEx, the same at any range] as the engine tells them; a null
        // reference otherwise [observed: GetCrosshairRefEx gives 00000000].
        "getcrosshairref" => XVal::Ref(g.senses.crosshair_ref.map_or(0, |r| r.0)),
        "getcrosshairrefex" => XVal::Ref(g.senses.crosshair_far.map_or(0, |r| r.0)),
        // `MinRange MaxRange`: the first object with collision the crosshair's ray meets
        // between the two distances, out to 50000 units [documented: ObjectUnderReticle];
        // the terrain is no object and no ray (a headless run) none [hypothesis].
        "objectunderreticle" => {
            let (min, max) = (num(args, 0, 0.0).max(0.0) as f32, (num(args, 1, 0.0) as f32).min(crate::functions::senses::RETICLE_REACH));
            let hit = g.senses.reticle.as_ref().filter(|_| max > min).and_then(|r| {
                let at = |d: f32| [r.start[0] + r.dir[0] * d, r.start[1] + r.dir[1] * d, r.start[2] + r.dir[2] * d];
                crate::ray::cast_hit(at(min), at(max), crate::ray::CastKind::World)
            });
            XVal::Ref(hit.and_then(|h| h.form).unwrap_or(0))
        }
        // [documented: GetCurrentObjective] the active quest's first objective shown and
        // not done [hypothesis: which objective counts as current], else -1 [hypothesis].
        "getcurrentobjective" => {
            let q = g.ui.active_quest.unwrap_or(FormId(0));
            let st = g.quest_state(q);
            XVal::Num(st.objectives.iter().find(|(_, o)| o.displayed && !o.completed).map_or(-1.0, |(&i, _)| f64::from(i)))
        }
        // [documented: GetDefaultForms; GetNthDefaultForm; SetNthDefaultForm] the DOBJ
        // list [hypothesis: the game's own systems keep reading the record's].
        "getdefaultforms" => {
            let entries = g.default_forms().into_iter().enumerate().map(|(i, f)| (XKey::Num(i as f64), XVal::Ref(f.map_or(0, |f| f.0)))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        "getnthdefaultform" => {
            let i = num(args, 0, -1.0);
            XVal::Ref(usize::try_from(i as i64).ok().and_then(|i| g.default_forms().get(i).copied().flatten()).map_or(0, |f| f.0))
        }
        "setnthdefaultform" => {
            if let (Ok(i), Some(f)) = (usize::try_from(num(args, 0, -1.0) as i64), form(args, 1)) {
                g.other.defaults.insert(i, f);
            }
            zero
        }
        // [documented: GetDoorTeleportX/Y/Z/Rot] where the load door leads, rotation in
        // degrees as GetAngle gives them [hypothesis]; 0 for any other door.
        "getdoorteleportx" | "getdoorteleporty" | "getdoorteleportz" | "getdoorteleportrot" => match subject.and_then(|d| g.door_teleport(d)) {
            Some((_, p, r)) => XVal::Num(f64::from(match name {
                "getdoorteleportx" => p[0],
                "getdoorteleporty" => p[1],
                "getdoorteleportz" => p[2],
                _ => r.to_degrees(),
            })),
            None => zero,
        },
        // [documented: SetDoorTeleport] `door [x y z rot]`: the left-out values are the
        // target's [hypothesis: the player still lands where the record says; the
        // getters see the change].
        "setdoorteleport" => {
            let (Some(d), Some(t)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let (p, r) = g.ref_pos_rot(t).unwrap_or(([0.0; 3], [0.0; 3]));
            let pos = [num(args, 1, f64::from(p[0])) as f32, num(args, 2, f64::from(p[1])) as f32, num(args, 3, f64::from(p[2])) as f32];
            let rot = args.get(4).map_or(r[2], |_| (num(args, 4, 0.0) as f32).to_radians());
            g.other.doors.insert(d, (t, pos, rot));
            zero
        }
        // [documented: GetNthExplicitRef; GetNumExplicitRefs] the script's reference list
        // (SCRO/SCRV), of the calling script when none is given.
        "getnthexplicitref" | "getnumexplicitrefs" => {
            let at = usize::from(name == "getnthexplicitref");
            let s = form(args, at).map(|f| if g.data.sig(f) == Some(*b"SCPT") { f } else { g.data.script_of_base(base(g, f)).unwrap_or(f) }).or(g.other.script);
            let refs: Vec<u32> = s
                .and_then(|s| g.data.script(s))
                .map(|c| c.unit.refs.iter().map(|r| if let fnv_script::unit::RefSlot::Form(f) = r { *f } else { 0 }).collect())
                .unwrap_or_default();
            if at == 0 {
                XVal::Num(refs.len() as f64)
            } else {
                XVal::Ref(usize::try_from(num(args, 0, -1.0) as i64).ok().and_then(|i| refs.get(i).copied()).unwrap_or(0))
            }
        }
        // [documented: GetParentCellOwningFactionRequiredRank] the rank the cell's owner
        // faction asks for; 0 without one [hypothesis].
        "getparentcellowningfactionrequiredrank" => {
            let rank = subject.and_then(|r| g.cell_of(r)).and_then(|c| g.cell_ownership(c)).and_then(|o| o.rank);
            XVal::Num(f64::from(rank.unwrap_or(0)))
        }
        // [documented: GetPermanent] on a reference, persistent; given one, a quest item
        // [observed: nvse-fifth, `GetPermanent rDoc` 1 though Doc is no quest item: the
        // reference's persistence counts too]. [documented: SetPermanent] sets the base
        // form's flag [hypothesis: the quest-item flag GetPermanent reads].
        "getpermanent" => match form(args, 0) {
            Some(r) => {
                let b = base(g, r);
                yes(g.world.is_quest_item(b) || g.persistent(r))
            }
            None => return crate::nvse_refs::call(g, "ispersistent", subject, &[]),
        },
        "setpermanent" => {
            let r = form(args, 1).or(subject).unwrap_or(FormId(0));
            let b = base(g, r);
            g.world.set_quest_object(b, num(args, 0, 0.0) != 0.0);
            zero
        }
        // [documented: GetTFC] the fly camera is on.
        "gettfc" => yes(g.ui.camera.fly_on),
        // [documented: HasScriptCommand] `opcode script [event]`: the script's code calls
        // the command (in that Begin block's event only, when given).
        "hasscriptcommand" => {
            let op = num(args, 0, -1.0);
            let Some(f) = form(args, 1) else { return Some(Ok(zero)) };
            let s = if g.data.sig(f) == Some(*b"SCPT") { Some(f) } else { g.data.script_of_base(base(g, f)) };
            let event = args.get(2).map(|_| num(args, 2, -1.0));
            let found = op >= 0.0
                && s.and_then(|s| g.data.script(s)).is_some_and(|c| {
                    c.program.blocks.iter().filter(|b| event.is_none_or(|e| f64::from(b.event) == e)).any(|b| serde_json::to_value(b).is_ok_and(|v| calls_opcode(&v, op as u64)))
                });
            yes(found)
        }
        // [documented: RemoveScript] the base form's script goes; gives it.
        // [documented: SetScript] `script [form]`: the base form runs that script; gives
        // the one before.
        "removescript" | "setscript" => {
            let set = name == "setscript";
            let new = if set { form(args, 0) } else { None };
            let b = form(args, usize::from(set)).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            if g.world.has_placed_temporary_base(b) {
                return Some(Err("script changes after temporary scene placement are not supported yet".into()));
            }
            let before = g.base_script(b);
            if before.is_some() || set {
                g.other.scripts.insert(b, new);
            }
            XVal::Ref(before.map_or(0, |s| s.0))
        }
        // [documented: RunScript] the script (or the scripted form's) runs once, with no
        // reference; true when it gave a true value [hypothesis: a script without a
        // Function block does not run, and gives 0].
        "runscript" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let s = if g.data.sig(f) == Some(*b"SCPT") { Some(f) } else { g.data.script_of_base(base(g, f)) };
            let r = s.map(|s| g.run_function(&XVal::Ref(s.0), None, Vec::new(), None, RunCtx::default()));
            yes(matches!(r, Some(Ok(v)) if v.truthy()))
        }
        // [documented: SetEditorID] for new forms; GetEditorID and EditorIDToFormID see it.
        "seteditorid" => {
            if let (Some(f), Some(XVal::Str(id))) = (form(args, 0), args.get(1)) {
                g.other.edids.insert(f, id.clone());
            }
            zero
        }
        // [documented: SetIsPowerArmor] `isPA [form]`: IsPowerArmor sees it [hypothesis:
        // equipping does not].
        "setispowerarmor" => {
            let f = form(args, 1).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            g.set_armo_power_armor(f, num(args, 0, 0.0) != 0.0);
            zero
        }
        // Formatted by the VM (`Vm::host_function`), then here.
        "setnthfactionranknameex" => {
            let (Some(XVal::Str(t)), Some(f)) = (args.first(), form(args, 1)) else { return Some(Ok(zero)) };
            if let Ok(i) = usize::try_from(num(args, 2, -1.0) as i64) {
                g.other.ranks.insert((f, i, num(args, 3, 0.0) != 0.0), t.clone());
            }
            zero
        }
        // [documented: Sv_Set; observed: nvse-fifth, `Sv_Set "n=%g" 3 sText` leaves the
        // string empty] it does nothing [hypothesis: the function is left over].
        "sv_set" => zero,
        // [documented: GetSoldItemInvRef; observed: outside an OnSell 00000000; OnSell is
        // not sent yet].
        "getsolditeminvref" => XVal::Ref(0),
        // [documented: GetPaired; observed: 0 in the base game, which has no tokens; no
        // item is a token here (nvse_tokens.rs)].
        "getpaired" => zero,
        // The game's folder, ending in "\" [observed: it ends with "Fallout New Vegas\"]:
        // the folder holding Data.
        "getfalloutdirectory" => {
            let dir = crate::mod_files::data_dir().and_then(|d| d.parent().map(|p| p.to_path_buf()));
            XVal::Str(dir.map(|d| format!("{}\\", d.display().to_string().trim_end_matches('\\'))).unwrap_or_default())
        }
        "getcurrentquestobjectiveteleportlinks" => objective_teleport_links(g, false),
        // [documented: OutputLocalMapPicturesOverride; SetOutputLocalMapPicturesGrids]
        // NVForever writes no local map pictures.
        "outputlocalmappicturesoverride" | "setoutputlocalmappicturesgrids" => zero,
        // [documented: SetPlayerCurrentAmmoRounds] the rounds in the player's magazine.
        "setplayercurrentammorounds" => {
            let n = num(args, 0, 0.0).max(0.0) as u32;
            g.other.set_magazine = Some(n);
            g.player_magazine = Some(n);
            zero
        }
        // The player-control locks [documented: DisablePlayerControlsAlt(Ex);
        // EnablePlayerControlsAlt(Ex); GetPlayerControlsDisabledAlt(Ex)]: each plugin keeps
        // its own; a group is locked while any plugin (or DisablePlayerControls) locks it.
        "disableplayercontrolsalt" | "disableplayercontrolsaltex" | "enableplayercontrolsalt" | "enableplayercontrolsaltex" => {
            let ex = name.ends_with("ex");
            let disable = name.starts_with("disable");
            let dflt = if disable { [true, true, true, true, false, false, false] } else { [true; 7] };
            let which = alt_groups(ex, args, dflt, dflt);
            let key = g.other.plugin.clone();
            let locks = g.other.alt_locks.entry(key.clone()).or_default();
            for (l, w) in locks.iter_mut().zip(which) {
                if w {
                    *l = disable;
                }
            }
            if !disable && locks.iter().all(|l| !l) {
                g.other.alt_locks.remove(&key);
            }
            zero
        }
        // GPCDEx answers as the Ex form [observed: #2430 probe, script_functions.py].
        "getplayercontrolsdisabledalt" | "getplayercontrolsdisabledaltex" | "gpcdex" => {
            let how = num(args, 0, 0.0) as i64;
            let which = alt_groups(name.ends_with("ex"), args.get(1..).unwrap_or(&[]), [true; 7], [false; 7]);
            let mut locked = [false; 7];
            for (p, l) in &g.other.alt_locks {
                if how >= 1 || *p == g.other.plugin {
                    for (o, x) in locked.iter_mut().zip(l) {
                        *o |= x;
                    }
                }
            }
            if how >= 2 {
                for (o, x) in locked.iter_mut().zip(g.player.controls_disabled) {
                    *o |= x;
                }
            }
            yes(locked.iter().zip(which).any(|(l, w)| *l && w))
        }
        // [documented: GetHotkeyItem; SetHotkeyItem, an item the player carries;
        // ClearHotkey] hotkeys 1–8 [hypothesis: NVForever has no hotkey wheel yet; these
        // keep the table].
        "gethotkeyitem" | "sethotkeyitem" | "clearhotkey" => {
            let Some(k) = usize::try_from(num(args, 0, 0.0) as i64 - 1).ok().filter(|&k| k < 8) else { return Some(Ok(zero)) };
            match name {
                "gethotkeyitem" => return Some(Ok(XVal::Ref(g.other.hotkeys[k].map_or(0, |f| f.0)))),
                "clearhotkey" => g.other.hotkeys[k] = None,
                _ => {
                    let item = form(args, 1).map(|f| base(g, f));
                    let carried = item.is_some_and(|i| g.world.inventory_mut(FormId::PLAYER_REF).is_ok_and(|inv| inv.count(i) > 0));
                    if carried {
                        g.other.hotkeys[k] = item;
                    }
                }
            }
            zero
        }
        // [documented: SetIsControl] a key made (or no longer) a custom control; IsControl
        // gives 2 for it.
        "setiscontrol" => {
            if let Ok(code) = u16::try_from(num(args, 0, -1.0) as i64) {
                if num(args, 1, 0.0) != 0.0 {
                    g.other.custom_controls.insert(code);
                } else {
                    g.other.custom_controls.remove(&code);
                }
            }
            zero
        }
        // [documented: GetOpenKey; SetOpenKey] the key of a lockable reference.
        "getopenkey" => {
            let r = subject.unwrap_or(FormId(0));
            let key = g.world.lock_of(r).and_then(|l| l.key);
            XVal::Ref(key.map_or(0, |k| k.0))
        }
        "setopenkey" => {
            let (Some(r), Some(k)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let _ = g.world.set_open_key(r, k);
            zero
        }
        // Packages [documented: AddPackageAt `package [index, -1 the end] [actor]`, the
        // index it went in at; RemovePackageAt `index [actor]`, the package now at that
        // index; RemoveAllPackages]. The list is the actor's base form's [hypothesis: as
        // the record's PKID list is], kept for the session.
        "addpackageat" | "removepackageat" | "removeallpackages" => {
            let a = form(args, if name == "addpackageat" { 2 } else { 1 }).or(subject).unwrap_or(FormId(0));
            let b = base(g, a);
            let mut list = g.package_list(a);
            let out = match name {
                "addpackageat" => {
                    let Some(p) = form(args, 0) else { return Some(Ok(zero)) };
                    // -1 appends and gives the index of the last package before it; an
                    // index past the end gives -1 and adds nothing [observed: nvse-seventh,
                    // Doc's 10 packages: -1 gave 9, 99 gave -1, 0 gave 0].
                    let i = num(args, 1, -1.0) as i64;
                    let n = list.len();
                    match usize::try_from(i) {
                        Ok(at) if at <= n => {
                            list.insert(at, p);
                            XVal::Num(at as f64)
                        }
                        Ok(_) => XVal::Num(-1.0),
                        Err(_) => {
                            list.push(p);
                            XVal::Num(n as f64 - 1.0)
                        }
                    }
                }
                "removepackageat" => {
                    let i = num(args, 0, -1.0) as i64;
                    let at = if i < 0 { list.len().checked_sub(1) } else { usize::try_from(i).ok().filter(|&i| i < list.len()) };
                    match at {
                        Some(at) => {
                            list.remove(at);
                            XVal::Ref(list.get(at).map_or(0, |p| p.0))
                        }
                        None => XVal::Ref(0),
                    }
                }
                _ => {
                    list.clear();
                    zero
                }
            };
            g.other.package_lists.insert(b, list);
            out
        }
        // [documented: GetPackageLocation; GetPackageLocationRadius] the package's first
        // location (PLDT): its reference, cell or object, and radius.
        "getpackagelocation" | "getpackagelocationradius" => {
            let loc = form(args, 0).and_then(|p| g.package_def(&p)).and_then(|d| d.location);
            if name == "getpackagelocationradius" {
                XVal::Num(loc.map_or(0.0, |l| f64::from(l.radius)))
            } else {
                use crate::package::LocationKind as K;
                let f = match loc.map(|l| l.kind) {
                    Some(K::NearReference(f) | K::InCell(f) | K::ObjectId(f)) => f,
                    _ => None,
                };
                XVal::Ref(f.map_or(0, |f| f.0))
            }
        }
        // [documented: SetPackageLocationRadius; SetPackageLocationReference;
        // SetPackageTargetReference, the calling reference when no target is given] the
        // package changes for every actor that runs it, for the session [hypothesis].
        "setpackagelocationradius" | "setpackagelocationreference" | "setpackagetargetreference" => {
            let Some(p) = form(args, 0) else { return Some(Ok(zero)) };
            use crate::package::{Location, LocationKind, Target, TargetKind};
            match name {
                "setpackagelocationradius" => {
                    let r = num(args, 1, 0.0) as i32;
                    g.change_package(p, |d| match d.location.as_mut() {
                        Some(l) => l.radius = r,
                        None => d.location = Some(Location { kind: LocationKind::NearCurrent, radius: r }),
                    });
                }
                "setpackagelocationreference" => {
                    let Some(t) = form(args, 1) else { return Some(Ok(zero)) };
                    g.change_package(p, |d| {
                        let radius = d.location.map_or(0, |l| l.radius);
                        d.location = Some(Location { kind: LocationKind::NearReference(Some(t)), radius });
                    });
                }
                _ => {
                    let Some(t) = form(args, 1).or(subject) else { return Some(Ok(zero)) };
                    g.change_package(p, |d| {
                        let count_distance = d.target.map_or(0, |x| x.count_distance);
                        d.target = Some(Target { kind: TargetKind::Reference(Some(t)), count_distance });
                    });
                }
            }
            zero
        }
        // Paths [documented: GetScopeModelPath; SetScopeModelPath; GetBipedIconPath;
        // SetModelPath(Ex), SetIconPath(Ex), SetTexturePath,
        // SetBipedModelPath(Ex), SetBipedIconPath(Ex)]: the scope model is MOD3 [observed:
        // the sniper rifle's MOD3 is interface\HUD\mildotscope.NIF]. The getters see the
        // change; the drawn models do not, but for a weapon's (weapon-mods.md §8)
        // [hypothesis: "If the replaced model is already loaded, then it will need to be
        // reloaded"; NVForever reads the record's].
        "getscopemodelpath" | "getbipediconpath" => {
            let (sub, at) = if name == "getscopemodelpath" {
                ("MOD3", 0)
            } else {
                // 0 and 2 the male icon, 1 and 3 the female [observed: nvse-seventh,
                // leather armour's codes 0 and 2 give the same icon].
                (match num(args, 0, -1.0) as i64 { 0 | 2 => "ICON", 1 | 3 => "ICO2", _ => "" }, 1)
            };
            let f = form(args, at).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            match crate::nvse::record_value(g, f, sub, "").or_else(|| crate::nvse::record_value(g, f, sub, "model")) {
                Some(XVal::Str(t)) if !sub.is_empty() => XVal::Str(t),
                _ => XVal::Str(String::new()),
            }
        }
        "setmodelpath" | "seticonpath" | "settexturepath" | "setscopemodelpath" | "setbipedmodelpath" | "setbipediconpath" => {
            let Some(XVal::Str(path)) = args.first().cloned() else { return Some(Ok(zero)) };
            let biped = name.starts_with("setbiped");
            let code = if biped { num(args, 1, -1.0) as i64 } else { 0 };
            let sub = match name {
                "setmodelpath" => "MODL",
                "setscopemodelpath" => "MOD3",
                "setbipedmodelpath" => match code { 0 => "MODL", 1 => "MOD3", 2 => "MOD2", 3 => "MOD4", _ => "" },
                "setbipediconpath" => match code { 0 | 2 => "ICON", 1 | 3 => "ICO2", _ => "" },
                _ => "ICON",
            };
            let f = form(args, if biped { 2 } else { 1 }).or(subject).map(|f| base(g, f));
            if let (Some(f), false) = (f, sub.is_empty()) {
                if sub == "MODL" && g.world.has_placed_temporary_base(f) {
                    return Some(Err("model replacement after temporary scene placement is not supported yet".into()));
                }
                g.field_overrides.insert((f, format!("{sub}/")), XVal::Str(path));
            }
            zero
        }
        // [documented: GetFlagsLow; GetFlagsHigh; SetFlagsLow; SetFlagsHigh] the record
        // header flags, low and high 16 bits, of the form (a reference's own).
        "getflagslow" | "getflagshigh" => {
            let f = form(args, 0).or(subject).unwrap_or(FormId(0));
            let v = g.record_flags(f);
            XVal::Num(f64::from(if name == "getflagslow" { v & 0xFFFF } else { v >> 16 }))
        }
        "setflagslow" | "setflagshigh" => {
            let f = form(args, 1).or(subject).unwrap_or(FormId(0));
            let m = (num(args, 0, 0.0) as i64 as u32) & 0xFFFF;
            let v = g.record_flags(f);
            let new = if name == "setflagslow" { (v & 0xFFFF_0000) | m } else { (v & 0xFFFF) | (m << 16) };
            g.other.flags.insert(f, new);
            zero
        }
        // [documented: GetHairFlags; GetEyesFlags; SetHairFlags `hair mask`;
        // SetEyesFlags `eyes mask`] the DATA flags (playable 1, not male 2, not female 4,
        // fixed colour 8 on hair).
        "gethairflags" | "geteyesflags" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            match crate::nvse::record_value(g, f, "DATA", "") {
                Some(XVal::Num(x)) => XVal::Num(x),
                _ => zero,
            }
        }
        "sethairflags" | "seteyesflags" => {
            if let Some(f) = form(args, 0) {
                g.field_overrides.insert((f, "DATA/".into()), XVal::Num(num(args, 1, 0.0).trunc()));
            }
            zero
        }
        // [documented: SetEquippedWeaponModFlags] the mods of the calling actor's
        // equipped weapon (1, 2, 4 for mods 1–3).
        "setequippedweaponmodflags" => {
            let a = subject.unwrap_or(FormId(0));
            let m = (num(args, 0, 0.0) as i64 as u32 & 7) as u8;
            if let Some(w) = crate::functions::items::equipped_weapon(g, a) {
                if let Ok(inv) = g.world.inventory_mut(a) {
                    let worn = inv.groups().find(|(i, e, _)| *i == w && e.equipped).map(|(_, e, _)| e);
                    if let Some(e) = worn {
                        inv.set_mods_one(w, e, m);
                    }
                }
            }
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}
