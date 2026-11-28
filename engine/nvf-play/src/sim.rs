//! The simulation as a resource: `fnv_sim::World` over the loaded plugins, its game clock
//! running in real time while the player is in the world.

use crate::{PlaySet, PlayState};
use bevy::prelude::*;
use fnv_esm::{LoadOrder, Schema};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_world::WorldIndex;

#[derive(Resource)]
pub struct SimState {
    pub forms: &'static FormStore,
    pub refs: &'static RefTable,
    pub world: World<'static>,
}

impl SimState {
    /// Build the simulation's tables from the plugins (they live for the whole run).
    pub fn new(lo: &LoadOrder, schema: &Schema, index: &WorldIndex) -> SimState {
        let mut forms = FormStore::build(lo, schema);
        let (files, applied) = fnv_game::stewie_inis::apply_modular_game_settings_to_forms(&mut forms);
        if files > 0 {
            info!("stewie: applied {applied} GMST override(s) from {files} modular GameSettings INI file(s)");
        }
        let forms: &'static FormStore = Box::leak(Box::new(forms));
        crate::pipboy::labels::set(&forms.settings);
        let refs: &'static RefTable = Box::leak(Box::new(RefTable::from_index(lo, schema, index)));
        SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) }
    }

    /// Build the simulation's tables applying Stewie modular `GameSettings\*.ini`
    /// overrides from explicit `roots` (without touching process-global `mod_files`
    /// roots).
    pub fn new_with_mod_roots(lo: &LoadOrder, schema: &Schema, index: &WorldIndex, roots: &[std::path::PathBuf]) -> SimState {
        let mut forms = FormStore::build(lo, schema);
        let _ = fnv_game::stewie_inis::apply_modular_game_settings_to_forms_from_roots(&mut forms, roots);
        let forms: &'static FormStore = Box::leak(Box::new(forms));
        crate::pipboy::labels::set(&forms.settings);
        let refs: &'static RefTable = Box::leak(Box::new(RefTable::from_index(lo, schema, index)));
        SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) }
    }

    /// The level `r` reads leveled lists at: fixed when its cell loaded, else what it would
    /// be fixed at now (`fnv_game::Game::leveled_at`). Read from this world, the one played:
    /// outside a game turn `game.world` is an empty stand-in (`script::with_world`), so
    /// `game.leveled_at` there would miss every fixed level.
    pub fn leveled_at(&self, game: &fnv_game::Game<'_>, r: FormId) -> fnv_sim::zones::LevelAt {
        let player = game.player.level.map_or(self.world.config().player_level, |l| l.clamp(1, i32::from(u16::MAX)) as u16);
        self.world.level_at_with_zone_lock(r, player, !game.tweaks.no_lock_encounter_zone_levels)
    }

    /// The seed and level `r`'s leveled picks are made with, as the game makes them in a
    /// turn ([`fnv_game::Game::pick_seed`], [`SimState::leveled_at`]): for the data's
    /// template lookups outside a turn (docs/spec-notes/actors.md §1.3).
    pub fn pick(&self, game: &fnv_game::Game<'_>, r: FormId) -> (FormId, fnv_sim::zones::LevelAt) {
        (game.pick_seed(r), self.leveled_at(game, r))
    }

    /// The base form of a reference as the simulation sees it now.
    pub fn base_of(&self, form_id: u32) -> Option<FormId> {
        let id = FormId(form_id);
        self.world.peek(id).map(|s| s.base).or_else(|| self.refs.get(id).map(|p| p.base))
    }

    /// Whether a reference is an actor: a placed ACHR or ACRE, or a reference created at
    /// run time (PlaceAtMe) of an actor base. Created actors are not among the placed
    /// references; tests of the placed table alone left them without a hit volume, so no
    /// shot could hit them (the stress tests' battle, 2026-10-18: the NCR troopers' rifle
    /// fire went through the created Legion raiders into the ground).
    pub fn is_actor(&self, form_id: u32) -> bool {
        let id = FormId(form_id);
        let placed = self.refs.get(id).map(|p| p.sig);
        let created = id.is_dynamic().then(|| self.world.peek(id).and_then(|s| self.forms.sig(s.base))).flatten();
        actor_ref(placed, created)
    }

    /// Whether a reference is a creature: a placed ACRE, or a reference created at run time
    /// (PlaceAtMe, PlaceLeveledActorAtMe) of a CREA or LVLC base, read as [`SimState::is_actor`]
    /// reads it. A created creature has no placement, so its record type alone told it
    /// from an NPC and it was offered Talk (#3124).
    pub fn is_creature(&self, form_id: u32) -> bool {
        let id = FormId(form_id);
        let placed = self.refs.get(id).map(|p| p.sig);
        let created = id.is_dynamic().then(|| self.world.peek(id).and_then(|s| self.forms.sig(s.base))).flatten();
        creature_ref(placed, created)
    }

    /// Whether a reference could pass [`SimState::is_actor`] now or later, whatever the
    /// simulation does: a placed ACHR or ACRE (a placed reference's record type never
    /// changes) or any id without a placement that is dynamic (a created reference's base
    /// is read from the world, and the id may be given to another base). Everything
    /// `is_actor` accepts passes this; the reverse is not so, it is no substitute for it.
    pub fn may_be_actor(&self, form_id: u32) -> bool {
        let id = FormId(form_id);
        match self.refs.get(id) {
            Some(p) => actor_ref(Some(p.sig), None),
            None => id.is_dynamic(),
        }
    }

    /// Display name of a base form (FULL), else its Editor ID.
    pub fn name(&self, base: FormId) -> Option<&str> {
        let b = self.forms.get(base)?;
        b.name.as_deref().or(b.edid.as_deref())
    }

    /// The name a placed reference is shown by: for an actor, its "Base Data" template
    /// source's ([`fnv_game::GameData::display_base`]: an actor placed from a template
    /// actor with no name of its own shows the name of the actor its template, through a
    /// leveled list, leads to; owner's playtest list 4, item 20); otherwise, and without
    /// the game data, its base form's ([`SimState::name`]). A name a script gave the base
    /// (SetActorFullName, [`fnv_game::Game::script_name`]) comes first: it renames every
    /// reference of the base [documented: GECK wiki SetActorFullName].
    pub fn ref_name<'s>(&'s self, game: Option<&'s fnv_game::Game<'_>>, form_id: u32) -> Option<&'s str> {
        // Another player of a co-op game goes by its player's name (none outside co-op).
        if let Some(n) = game.and_then(|g| g.coop_actors.get(&FormId(form_id))) {
            return Some(n);
        }
        let base = self.base_of(form_id)?;
        let shown = self.shown_base(game, base, form_id);
        let renamed = game.and_then(|g| g.script_name(base).or_else(|| g.script_name(shown)));
        renamed.or_else(|| self.name(shown)).or_else(|| self.name(base))
    }

    /// [`SimState::ref_name`] for the screen: the name the data (or a script) gives, never
    /// an Editor ID. `None` for a reference with no name anywhere in its chain, which is
    /// then shown without one (#3445: a Lonesome Road deathclaw placed from the nameless
    /// `NVDLC04DeathclawAmbusherLVL` was named by that Editor ID in VATS).
    pub fn ref_shown_name<'s>(&'s self, game: Option<&'s fnv_game::Game<'_>>, form_id: u32) -> Option<&'s str> {
        let base = self.base_of(form_id)?;
        let shown = self.shown_base(game, base, form_id);
        let full = |id: FormId| self.forms.get(id).and_then(|b| b.name.as_deref()).filter(|n| !n.is_empty());
        let renamed = game.and_then(|g| g.script_name(base).or_else(|| g.script_name(shown)));
        renamed.or_else(|| full(shown)).or_else(|| full(base))
    }

    /// The actor base whose name the reference `form_id` (placed with `base`) is shown
    /// by: the "Base Data" source of its template chain, its leveled lists read with the
    /// seed and at the level the drawn actor's are ([`SimState::pick`]). `base` itself
    /// without the game data or a chain to follow.
    fn shown_base(&self, game: Option<&fnv_game::Game<'_>>, base: FormId, form_id: u32) -> FormId {
        game.map_or(base, |g| {
            let (seed, at) = self.pick(g, FormId(form_id));
            g.runtime_template_source(base, seed, at, |d| d.template_base).map_or(base, |(id, _)| id)
        })
    }

    /// A placed reference's pose in the simulation (position, Euler angles in radians)
    /// when it differs from its placement: moved by physics (pushed or knocked clutter,
    /// `crate::phys::PhysState::settle_placed`) or by a script. `None` for references the
    /// plugins do not place (the player, dropped items) and for those still where the
    /// plugins put them.
    pub fn moved_pose(&self, form_id: u32) -> Option<([f32; 3], [f32; 3])> {
        let id = FormId(form_id);
        let (s, p) = (self.world.peek(id)?, self.refs.get(id)?);
        (s.pos != p.pos || s.rot != p.rot).then_some((s.pos, s.rot))
    }

    /// The scale a reference is drawn and collides at: a script's (SetScale), else its
    /// placement's (XSCL), else 1; never below [`MIN_DRAWN_SCALE`].
    pub fn ref_scale(&self, form_id: u32) -> f32 {
        let id = FormId(form_id);
        self.script_scale(form_id).or_else(|| self.refs.get(id).and_then(|p| p.scale)).unwrap_or(1.0).max(MIN_DRAWN_SCALE)
    }

    /// The scale a script set on a reference (SetScale), if any.
    pub fn script_scale(&self, form_id: u32) -> Option<f32> {
        self.world.peek(FormId(form_id)).and_then(|s| s.scale)
    }

    /// Whether a placed reference is in the world now (not taken or deleted, enabled).
    pub fn present(&self, form_id: u32) -> bool {
        let id = FormId(form_id);
        !self.world.peek(id).is_some_and(|s| s.deleted()) && self.world.is_enabled(id).unwrap_or(true)
    }
}

#[cfg(test)]
impl SimState {
    /// A simulation of these placed references, with no base forms (no game data).
    pub(crate) fn over_placed(placed: impl IntoIterator<Item = fnv_sim::refs::PlacedRef>) -> SimState {
        let mut refs = RefTable::default();
        refs.refs.extend(placed.into_iter().map(|p| (p.id, p)));
        let forms: &'static FormStore = Box::leak(Box::default());
        let refs: &'static RefTable = Box::leak(Box::new(refs));
        SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) }
    }
}

/// A placed reference of nothing in particular, for [`SimState::over_placed`].
#[cfg(test)]
pub(crate) fn placed_ref(id: u32) -> fnv_sim::refs::PlacedRef {
    fnv_sim::refs::PlacedRef {
        id: FormId(id),
        sig: *b"REFR",
        edid: None,
        base: FormId(0x100),
        cell: Some(FormId(0x200)),
        stored_in: Some(FormId(0x200)),
        plugin: 0,
        flags: 0,
        in_persistent_group: false,
        pos: [0.0; 3],
        rot: [0.0; 3],
        scale: None,
        count: None,
        ammo: None,
        health: None,
        owner: None,
        lock: None,
        teleport: None,
        enable_parent: None,
        linked: None,
        patrol_idle: None,
        patrol_topic: None,
        merchant_container: None,
        marker_reputation: None,
        open_by_default: false,
        primitive: None,
        activate_parents: Vec::new(),
        parent_activate_only: false,
        prompt: None,
        radiation: None,
        radius: None,
        encounter_zone: None,
        level_modifier: None,
        map_marker: None,
        radio: None,
    }
}

/// The smallest scale a reference is drawn at: `SetScale 0` (an artillery piece the
/// scripts hide that way, FalloutNV.esm SCPT 0x001043CC) draws it this small, too small
/// to see, so that the drawn transform stays invertible; below it the reference has no
/// collision (`crate::phys`) [design].
pub const MIN_DRAWN_SCALE: f32 = 1e-3;

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        // Before the scripts, so a frame's scripts all see that frame's game time (one
        // order every frame; which comes first in the game is [hypothesis]: the capture of
        // docs/spec-notes/layer5-captures.md §9 only shows a quest script's own clock
        // change reaching object scripts in the same frame).
        app.add_systems(Update, advance_clock.run_if(in_state(PlayState::Playing)).before(PlaySet::Scripts));
    }
}

/// Game time runs at TimeScale while the player is in the world, not in the container
/// and inventory menus (`PlayState::Menu`) [observed for the container menu: the game
/// clock stopped there, docs/spec-notes/layer5-captures.md §8.3 e; it kept running in the
/// console and Wait menus, docs/spec-notes/simulation.md §5b]. It stops in a conversation
/// with the player as well (`PlayState::Dialogue`) [owner ruling 2026-10-10: a
/// conversation with the player pauses the game; `crate::dialogue`].
fn advance_clock(time: Res<Time>, mut sim: ResMut<SimState>) {
    if let Err(e) = sim.world.advance_real_seconds(time.delta_secs_f64()) {
        warn_once!("game clock: {e}");
    }
}

/// [`SimState::is_actor`]'s rule: a placed reference by its record type (ACHR, ACRE), a
/// created one by its base's (NPC_, CREA, or a leveled list of them).
pub fn actor_ref(placed_sig: Option<[u8; 4]>, created_base_sig: Option<[u8; 4]>) -> bool {
    match (placed_sig, created_base_sig) {
        (Some(s), _) => matches!(&s, b"ACHR" | b"ACRE"),
        (None, Some(b)) => matches!(&b, b"NPC_" | b"CREA" | b"LVLN" | b"LVLC"),
        (None, None) => false,
    }
}

/// Whether a reference is a creature, as [`actor_ref`] tells an actor: by its placed record
/// type (ACRE), else by its created base's (CREA, or the LVLC it was made from).
pub fn creature_ref(placed_sig: Option<[u8; 4]>, created_base_sig: Option<[u8; 4]>) -> bool {
    match (placed_sig, created_base_sig) {
        (Some(s), _) => &s == b"ACRE",
        (None, Some(b)) => matches!(&b, b"CREA" | b"LVLC"),
        (None, None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #3124: a created creature (PlaceAtMe of a CREA, PlaceLeveledActorAtMe of an LVLC) is a
    /// creature as a placed ACRE is; a created NPC, a placed ACHR and an item are not.
    #[test]
    fn created_creatures_are_creatures_as_placed_ones_are() {
        assert!(creature_ref(Some(*b"ACRE"), None));
        assert!(!creature_ref(Some(*b"ACHR"), None) && !creature_ref(Some(*b"REFR"), None));
        assert!(creature_ref(None, Some(*b"CREA")) && creature_ref(None, Some(*b"LVLC")));
        assert!(!creature_ref(None, Some(*b"NPC_")) && !creature_ref(None, Some(*b"LVLN")) && !creature_ref(None, Some(*b"MISC")) && !creature_ref(None, None));

        // Over a simulation: a creature and an NPC made at run time (bases known by their
        // signatures alone, as hitvol's tests make them).
        let mut forms = FormStore::default();
        forms.sigs.insert(FormId(0x10), *b"CREA");
        forms.sigs.insert(FormId(0x11), *b"NPC_");
        let forms: &'static FormStore = Box::leak(Box::new(forms));
        let refs: &'static RefTable = Box::leak(Box::default());
        let mut sim = SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) };
        let creature = sim.world.place(FormId(0x10), None, [0.0; 3], [0.0; 3], 1).unwrap().0;
        let npc = sim.world.place(FormId(0x11), None, [0.0; 3], [0.0; 3], 1).unwrap().0;
        assert!(sim.is_creature(creature) && sim.is_actor(creature));
        assert!(!sim.is_creature(npc) && sim.is_actor(npc));
    }

    /// #3445: the screen never shows an Editor ID as a reference's name.
    #[test]
    fn a_nameless_base_is_shown_without_a_name() {
        use fnv_sim::forms::{ActorBase, BaseData, BaseForm};
        let actor = |id: u32, name: Option<&str>| BaseForm {
            id: FormId(id),
            sig: *b"CREA",
            edid: Some(format!("Edid{id:X}")),
            name: name.map(str::to_string),
            script: None,
            prompt: None,
            voice_filter: false,
            data: BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None }),
        };
        let sim = test_world::sim(vec![actor(0x10, None), actor(0x11, Some("Deathclaw")), actor(0x12, Some(""))], vec![test_world::placed(0x20, 0x10), test_world::placed(0x21, 0x11), test_world::placed(0x22, 0x12)]);
        // The log's name falls back on the Editor ID; the screen's does not.
        assert_eq!(sim.ref_name(None, 0x20), Some("Edid10"));
        assert_eq!(sim.ref_shown_name(None, 0x20), None);
        assert_eq!(sim.ref_shown_name(None, 0x21), Some("Deathclaw"));
        assert_eq!(sim.ref_shown_name(None, 0x22), None);
        assert_eq!(sim.ref_shown_name(None, 0x99), None);
    }

    #[test]
    fn created_actors_are_actors_as_placed_ones_are() {
        assert!(actor_ref(Some(*b"ACHR"), None) && actor_ref(Some(*b"ACRE"), None));
        assert!(!actor_ref(Some(*b"REFR"), None));
        // PlaceAtMe of an NPC, a creature or a leveled list of either.
        for base in [b"NPC_", b"CREA", b"LVLN", b"LVLC"] {
            assert!(actor_ref(None, Some(*base)), "{base:?}");
        }
        // A created item or container is not.
        assert!(!actor_ref(None, Some(*b"MISC")) && !actor_ref(None, Some(*b"CONT")) && !actor_ref(None, None));
    }

    #[test]
    fn new_with_mod_roots_applies_modular_game_settings_without_touching_vanilla_when_absent() {
        fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = sig.to_vec();
            out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            out.extend_from_slice(data);
            out
        }
        fn rec(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
            let body: Vec<u8> = subs.iter().flatten().copied().collect();
            let mut out = sig.to_vec();
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&body);
            out
        }
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = b"GRUP".to_vec();
            out.extend_from_slice(&((body.len() + 24) as u32).to_le_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(body);
            out
        }

        let tes4 = rec(b"TES4", 0, &[sub(b"HEDR", &[0u8; 12])]);
        let gmsts = [
            rec(b"GMST", 0x201, &[sub(b"EDID", b"fSimModularFloat\0"), sub(b"DATA", &4.5f32.to_le_bytes())]),
            rec(b"GMST", 0x202, &[sub(b"EDID", b"iSimModularInt\0"), sub(b"DATA", &3i32.to_le_bytes())]),
            rec(b"GMST", 0x203, &[sub(b"EDID", b"sSimModularLabel\0"), sub(b"DATA", b"Vanilla\0")]),
        ]
        .concat();
        let bytes = [tes4, top(b"GMST", &gmsts)].concat();
        let plugins = [fnv_esm::Plugin::parse(&bytes, "SimModularGmst.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap();
        let index = WorldIndex::default();

        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("nvf-sim-gmst-{}-{stamp}", std::process::id()));
        let empty = root.join("EmptyData");
        let modded = root.join("ModData");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(modded.join("NVSE/Plugins/Tweaks/GameSettings")).unwrap();
        std::fs::write(
            modded.join("NVSE/Plugins/Tweaks/GameSettings/10_override.ini"),
            "[GameSettings]\nfSimModularFloat = 18.25\niSimModularInt = 11\nsSimModularLabel = \"Modded\"\n",
        )
        .unwrap();

        let vanilla = SimState::new_with_mod_roots(&lo, &schema, &index, &[empty]);
        assert_eq!(vanilla.world.setting_f32("fSimModularFloat"), Some(4.5));
        assert_eq!(vanilla.world.setting_f32("iSimModularInt"), Some(3.0));
        assert_eq!(vanilla.world.setting_value("sSimModularLabel"), Some(&fnv_sim::settings::Setting::Str("Vanilla".into())));

        let overridden = SimState::new_with_mod_roots(&lo, &schema, &index, &[modded]);
        assert_eq!(overridden.world.setting_f32("fSimModularFloat"), Some(18.25));
        assert_eq!(overridden.world.setting_f32("iSimModularInt"), Some(11.0));
        assert_eq!(overridden.world.setting_value("sSimModularLabel"), Some(&fnv_sim::settings::Setting::Str("Modded".into())));

        let _ = std::fs::remove_dir_all(&root);
    }
}

/// A simulation made by hand for tests: forms and references in memory, no game data.
#[cfg(test)]
pub(crate) mod test_world {
    use super::*;
    use fnv_sim::forms::{ActorBase, BaseData, BaseForm, Door, Item, ItemKind};
    use fnv_sim::refs::PlacedRef;

    fn base(id: u32, data: BaseData) -> BaseForm {
        BaseForm { id: FormId(id), sig: *b"MISC", edid: Some(format!("Edid{id:X}")), name: None, script: None, prompt: None, voice_filter: false, data }
    }

    pub(crate) fn item_base(id: u32) -> BaseForm {
        base(id, BaseData::Item(Item { kind: ItemKind::Aid, value: Some(1), money: None, weight: Some(0.5), slots: None, projectile: None, health: None, non_playable: false, cant_drop: false, armor_class: Default::default(), power_armor: false, armor_dr: None, armor_dt: None, weapon_damage: None, weight_mods: [0.0; 3], drawable: false, not_for_npcs: false, thrown: false, quest_item: false, icon: None, placed_count: None, teaches: None, cant_be_taken: false, modulates_voice: false, weapon_traits: None }))
    }

    pub(crate) fn door_base(id: u32) -> BaseForm {
        base(id, BaseData::Door(Door { automatic: false, minimal_use: false }))
    }

    /// A plain reference of `base` nowhere in particular.
    pub(crate) fn placed(id: u32, base: u32) -> PlacedRef {
        PlacedRef {
            id: FormId(id),
            sig: *b"REFR",
            edid: None,
            base: FormId(base),
            cell: None,
            stored_in: None,
            plugin: 0,
            flags: 0,
            in_persistent_group: false,
            pos: [0.0; 3],
            rot: [0.0; 3],
            scale: None,
            count: None,
            ammo: None,
            health: None,
            owner: None,
            lock: None,
            teleport: None,
            enable_parent: None,
            linked: None,
            patrol_idle: None,
            patrol_topic: None,
            merchant_container: None,
            marker_reputation: None,
            open_by_default: false,
            primitive: None,
            activate_parents: Vec::new(),
            parent_activate_only: false,
            prompt: None,
            radiation: None,
            radius: None,
            encounter_zone: None,
            level_modifier: None,
            map_marker: None,
            radio: None,
        }
    }

    /// These bases (and the player's) and references.
    pub(crate) fn sim(bases: Vec<BaseForm>, placed: Vec<PlacedRef>) -> SimState {
        let mut forms = FormStore::default();
        for b in bases.into_iter().chain([base(FormId::PLAYER_BASE.0, BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None }))]) {
            // `World::place` knows a form by its signature.
            forms.sigs.insert(b.id, b.sig);
            forms.bases.insert(b.id, b);
        }
        let mut refs = RefTable::default();
        for r in placed {
            // In its cell's list, as `RefTable::from_index` puts it (what a cell reset
            // walks).
            if let Some(c) = r.cell {
                refs.by_cell.entry(c).or_default().push(r.id);
            }
            refs.refs.insert(r.id, r);
        }
        let forms: &'static FormStore = Box::leak(Box::new(forms));
        let refs: &'static RefTable = Box::leak(Box::new(refs));
        SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) }
    }
}
