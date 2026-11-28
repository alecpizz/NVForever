//! fnvtool — the parity lab's candidate CLI. Every command prints one JSON
//! document on stdout (schemas in docs/cli-contract.md) and diagnostics on stderr.
//! Exit codes: 0 success, 1 the input could not be processed, 2 usage error.

mod activator_models;
mod actor_anims;
mod actor_creature_weapons;
mod ai_choice;
mod ai_cmd;
mod ai_packages;
mod ai_run;
mod actor_furniture;
mod actor_locomotion;
mod actor_tri;
mod actor_detect;
mod actor_fidgets;
mod actor_gestures;
mod actor_factions;
mod actor_karma;
mod actor_values;
mod actor_aid;
mod actor_chargen;
mod actor_creator;
mod actor_weapons;
mod actor_cmd;
mod bsa_cmd;
mod collide_cmd;
mod crash_report;
mod combat_cmd;
mod combat_sim;
mod coop_cmd;
mod dds_cmd;
mod mo2_args;
mod mod_log_digest;
mod runner_diagnostics;
mod dialogue_cmd;
mod spt_cmd;
mod esm_cmd;
mod esm_fields;
mod ext_cmd;
mod install_cmd;
mod nav_cmd;
mod nav_path;
mod observer;
mod nif_cmd;
mod pbr_check;
mod pbr_cmd;
mod phys_cmd;
mod quest_sweep;
mod save_cmd;
mod script_cmd;
mod sim_cmd;
mod source;
mod ui_cmd;
mod voice_cmd;
mod world_alt_textures;
mod world_audio;
mod world_posed_clutter;
mod world_radio;
mod world_terminals;
mod world_cmd;

use clap::{Args, Parser, Subcommand};
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;

/// NVForever (FNV engine reimplementation) inspection CLI.
#[derive(Parser)]
#[command(name = "fnvtool", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Record the game install (read-only) in NVForever.ini and create NVForever's own
    /// home; refused when the game lacks files NVForever needs.
    Setup {
        /// The Fallout New Vegas folder (or its Data folder).
        #[arg(long)]
        game: PathBuf,
        #[command(flatten)]
        home: HomeArg,
    },
    /// Show where NVForever keeps its files.
    Paths {
        #[command(flatten)]
        home: HomeArg,
    },
    /// BSA archives.
    #[command(subcommand)]
    Bsa(BsaCmd),
    /// The virtual file system: archives and loose files as the game mounts them.
    #[command(subcommand)]
    Vfs(VfsCmd),
    /// ESM/ESP plugins.
    #[command(subcommand)]
    Esm(EsmCmd),
    /// NIF meshes and KF animations.
    #[command(subcommand)]
    Nif(NifCmd),
    /// DDS textures.
    #[command(subcommand)]
    Dds(DdsCmd),
    /// Authored PBR material maps under `textures\pbr\` (docs/spec-notes/pbr-materials.md).
    #[command(subcommand)]
    Pbr(pbr_cmd::PbrCmd),
    /// SpeedTree .spt tree parameter files.
    #[command(subcommand)]
    Spt(SptCmd),
    /// Worldspaces, cells, placed references and landscape across a load order.
    #[command(subcommand)]
    World(WorldCmd),
    /// Physics world built from the game data: terrain, collision, probe characters.
    #[command(subcommand)]
    Phys(PhysCmd),
    /// Layer 4 collision geometry: Havok collision blocks of meshes.
    #[command(subcommand)]
    Collide(CollideCmd),
    /// Layer 4 core simulation: forms, references, inventories, activation.
    #[command(subcommand)]
    Sim(SimCmd),
    /// Layer 5 compiled scripts: units, bytecode IR, canonical text, opcode coverage.
    #[command(subcommand)]
    Script(script_cmd::ScriptCmd),
    /// Layer 6 dialogue: topics, lines and headless conversations.
    #[command(subcommand)]
    Dialogue(dialogue_cmd::DialogueCmd),
    /// Quests: static checks over the data for soft locks (issue #2365).
    #[command(subcommand)]
    Quest(quest_sweep::QuestCmd),
    /// Co-op data: the unique quest rewards of a load order (shared progression).
    #[command(subcommand)]
    Coop(coop_cmd::CoopCmd),
    /// Actor appearance: the parts that make up placed NPCs (workstream 5).
    #[command(subcommand)]
    Actor(ActorCmd),
    /// Layer 6 combat data: weapons, ammo, projectiles, explosions, body parts.
    #[command(subcommand)]
    Combat(combat_cmd::CombatCmd),
    /// Navmeshes: records, the nav graph, paths and path-following walks.
    #[command(subcommand)]
    Nav(nav_cmd::NavCmd),
    /// AI packages (Layer 6): records, package lists, choice.
    #[command(subcommand)]
    Ai(ai_cmd::AiCmd),
    /// Dialogue voice files: a line's sound path and length, coverage over a load order.
    #[command(subcommand)]
    Voice(voice_cmd::VoiceCmd),
    /// Script-extender use in a load order's scripts: functions, syntax, units without
    /// source (counts and names only).
    #[command(subcommand)]
    Ext(ext_cmd::ExtCmd),
    /// The menu files read into the tile tree: counts and mended spots over all menus, one
    /// menu's tiles and trait values.
    #[command(subcommand)]
    Ui(ui_cmd::UiCmd),
    /// Offline `.fos` save inspection and conversion to NVForever `.nvfsave` files.
    #[command(subcommand)]
    Save(save_cmd::SaveCmd),
}

#[derive(Subcommand)]
pub enum ActorCmd {
    /// Actor values (docs/spec-notes/actor-values.md): the 77 values of each named
    /// actor as its record and the formulas give them in a game where nothing has
    /// happened; with --survey, what the NPC_ and CREA records hold, how the skill
    /// formula fits the stored skills for each attribute, and the settings read
    /// (fnv.actor.values/1).
    Values {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// `player`, a placed actor, or an actor base (its first placed reference), by
        /// Editor ID or load-order FormID (repeatable).
        #[arg(long)]
        actor: Vec<String>,
        #[arg(long)]
        survey: bool,
    },
    /// Aid items (docs/spec-notes/aid-items.md §8): every ingestible (ALCH) with its
    /// flags, addiction data and decoded effects, counts over them, and with --use what
    /// using items does to the player in the authored game (fnv.actor.aid/1).
    Aid {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// List only these items (Editor ID or FormID; repeatable).
        #[arg(long)]
        item: Vec<String>,
        /// Use these items on the player, in order (repeatable).
        #[arg(long = "use")]
        uses: Vec<String>,
        /// Damage before the uses, `<actor value>:<amount>` (repeatable).
        #[arg(long)]
        damage: Vec<String>,
        /// Seconds of play after the uses.
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
        /// Hardcore mode on.
        #[arg(long)]
        hardcore: bool,
        /// Perks the player has before the uses (Editor ID or FormID; repeatable).
        #[arg(long)]
        perk: Vec<String>,
        /// Hours waited after the seconds of play (effects and needs, aid-items.md §6, §11).
        #[arg(long, default_value_t = 0.0)]
        wait: f64,
        /// Sleep those hours instead of waiting them.
        #[arg(long)]
        sleep: bool,
        /// Set actor values before the uses, `<actor value>:<value>` (repeatable).
        #[arg(long)]
        set: Vec<String>,
        /// Also list the perks with entry point effects and the stages of rads and needs.
        #[arg(long)]
        perks: bool,
    },
    /// Character creation (docs/spec-notes/chargen.md §6): the name offered, the
    /// S.P.E.C.I.A.L. sheet, the tag-skill list with values and the traits with their
    /// requirement lines, as a new player sees them (fnv.actor.chargen/1).
    Chargen {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// Seven attributes, Strength to Luck, comma-separated, applied before the
        /// skills are listed (every point must be given out).
        #[arg(long)]
        special: Option<String>,
        /// Skills to tag (repeatable, by actor value name).
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Factions of actors (docs/spec-notes/factions.md): for each named actor its
    /// template chain, the factions the records give it and how it stands towards the
    /// player; with --survey, the "Factions" template flag, templates that are leveled
    /// lists, the two directions of faction reactions and the placed actors' stance
    /// towards the player (fnv.actor.factions/1).
    Factions {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// `player`, a placed actor, or an actor base (its first placed reference), by
        /// Editor ID or load-order FormID (repeatable).
        #[arg(long)]
        actor: Vec<String>,
        #[arg(long)]
        survey: bool,
    },
    /// Karma and crime (docs/spec-notes/crime-karma.md): the settings and globals the
    /// rules read; for each named actor its karma, alignment and what killing it does to
    /// the player's karma; for each named cell its owner and what is owned in it; with
    /// --survey, alignments of records and placed actors, faction flags (Evil, Track
    /// Crime), owned and public cells and owned references (fnv.actor.karma/1).
    Karma {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// `player`, a placed actor, or an actor base (its first placed reference), by
        /// Editor ID or load-order FormID (repeatable).
        #[arg(long)]
        actor: Vec<String>,
        /// A cell by Editor ID (repeatable).
        #[arg(long)]
        cell: Vec<String>,
        #[arg(long)]
        survey: bool,
    },
    /// Fidgets from the IDLE tree: drawn-weapon ones (docs/spec-notes/idles.md §11),
    /// each actor's weapon put out in a game where nothing has happened, with
    /// `--weapon-away` those of a person standing with its weapon away (§11.4), or with
    /// `--furniture` the ones on furniture (furniture.md §8); the tree asked `--checks`
    /// times, the picks counted per idle (fnv.actor.fidgets/1).
    Fidgets {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// Placed actors or actor bases, by Editor ID or FormID, or `player` (repeatable).
        #[arg(long, required = true)]
        actor: Vec<String>,
        /// The weapon in every actor's hands, by Editor ID or FormID (none is equipped
        /// in a fresh game).
        #[arg(long)]
        weapon: Option<String>,
        /// How many times the tree is asked for each actor.
        #[arg(long, default_value_t = 1000)]
        checks: u32,
        /// Put every actor on this furniture instead, as `REF:MARKER_ID` (a placed
        /// furniture reference by Editor ID or FormID, and the ID of the model marker it
        /// sat down by): the fidgets on furniture are asked for, not the drawn-weapon ones.
        #[arg(long)]
        furniture: Option<String>,
        /// The player's view is first person (`IsPC1stPerson`).
        #[arg(long)]
        first_person: bool,
        /// Leave every actor's weapon away instead: the fidgets of a person standing
        /// with its weapon away are asked for (idles.md §11.4), not the drawn-weapon ones.
        #[arg(long, conflicts_with_all = ["weapon", "furniture"])]
        weapon_away: bool,
    },
    /// Talking gestures (docs/spec-notes/idles.md §12): the player starts a
    /// conversation with each actor in a game where nothing has happened, and the IDLE
    /// tree is asked `--checks` times while it talks and as many while it listens; the
    /// picks counted per idle and group (fnv.actor.gestures/1).
    Gestures {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// Placed actors or actor bases, by Editor ID or FormID (repeatable).
        #[arg(long, required = true)]
        actor: Vec<String>,
        /// The line the actor says (INFO by Editor ID or FormID): its mood; default none
        /// (Neutral).
        #[arg(long)]
        line: Option<String>,
        /// How many times the tree is asked for each actor and part.
        #[arg(long, default_value_t = 1000)]
        checks: u32,
    },
    /// Detection numbers (docs/design/detection.md §4): what one actor's records put
    /// into the detection formula against a target, and the value and band (hidden,
    /// noticed, detected) in a set of situations at several distances, with the farthest
    /// distance of each band (fnv.actor.detect/1).
    Detect {
        #[command(flatten)]
        w: WorldArgs,
        /// Script function catalogue (default: `script/functions.json` next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// The actor detecting: a placed actor or an actor base, by Editor ID or FormID.
        #[arg(long)]
        detector: String,
        /// The actor detected (default: the player).
        #[arg(long, default_value = "player")]
        target: String,
        /// Measure as in an interior (the shorter reach).
        #[arg(long)]
        interior: bool,
        /// The light level of a lit place, 0–100 (default: the original's at Goodsprings
        /// at noon, docs/design/detection.md §3).
        #[arg(long, default_value_t = 100.0)]
        light: f64,
        /// The light level of a dark place, 0–100 (default: the original's at Goodsprings
        /// at midnight, 210/255; a cave is about 36, a dust storm 0).
        #[arg(long, default_value_t = 82.35)]
        dark: f64,
    },
    /// The parts list of placed NPCs: skeleton, race parts, hair, head parts, worn
    /// armour and FaceGen coefficients (fnv.actor.appearance/1).
    #[command(group(clap::ArgGroup::new("which").required(true).args(["reference", "cell", "all"])))]
    Appearance {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// One placed actor (ACHR FormID).
        #[arg(long = "ref", value_parser = parse_formid)]
        reference: Option<u32>,
        /// Every ACHR in a cell (Editor ID or FormID).
        #[arg(long)]
        cell: Option<String>,
        /// Every ACHR in the load order.
        #[arg(long)]
        all: bool,
        /// Only actors whose appearance involves no random leveled choice (`random`
        /// false): the part an independent implementation can reproduce exactly.
        #[arg(long)]
        deterministic: bool,
    },
    /// The character creator (docs/spec-notes/character-creator.md §7): playable races
    /// with their hairs, eyes and starting faces, beards, hair colours, and the named
    /// sliders of `facegen\si.ctl` with their ranges and effects (fnv.actor.creator/1).
    Creator {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
    },
    /// What the appearance loaders read, with observational statistics on templates,
    /// skeletons, races and FaceGen data (fnv.actor.forms/1).
    Forms {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
    },
    /// Placed creatures (ACRE): skeleton, NIFZ body models, base scale and KFFZ list
    /// after leveled and template resolution (fnv.actor.creatures/1); with
    /// `--check-meshes`, whether each model joins its skeleton.
    #[command(group(clap::ArgGroup::new("which").required(true).args(["reference", "cell", "all"])))]
    Creatures {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// One placed creature (ACRE FormID).
        #[arg(long = "ref", value_parser = parse_formid)]
        reference: Option<u32>,
        /// Every ACRE in a cell (Editor ID or FormID).
        #[arg(long)]
        cell: Option<String>,
        /// Every ACRE in the load order.
        #[arg(long)]
        all: bool,
        /// Only creatures whose resolution involves no random leveled choice.
        #[arg(long)]
        deterministic: bool,
        /// Also load every (skeleton, model) pair and report skin bones and parent
        /// bones the skeleton lacks, and missing files (adds `meshes`).
        #[arg(long)]
        check_meshes: bool,
        /// NIF schema directory for --check-meshes (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
    },
    /// For every placed NPC: do its part meshes join its skeleton (skin bones and `Prn`
    /// bones present), and do its models and textures exist (fnv.actor.meshes/1)?
    Meshes {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
    },
    /// Animation data catalogue: every `.kf` grouped by skeleton folder with its
    /// sequences and how many tracks bind to that skeleton, IDLE records per animation
    /// group with the `.kf` they name (found / missing), and creature KFFZ lists
    /// (fnv.actor.anims/1).
    Anims {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Script function catalogue for naming condition functions (default:
        /// `script/functions.json` next to --schema; names are left out if absent).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// Leave out the per-file and per-IDLE lists (summary and groups only).
        #[arg(long)]
        summary_only: bool,
    },
    /// Weapon stance clips of a skeleton folder (equip, unequip, holster, aim: `prn:` keys,
    /// track priorities, holster and hand bones) and the weapon each placed NPC draws
    /// (fnv.actor.weapons/1).
    Weapons {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Skeleton folder (VFS path).
        #[arg(long, default_value = r"meshes\characters\_male")]
        folder: String,
    },
    /// Creature weapons (docs/spec-notes/actors.md §10.4): for every creature base placed
    /// creatures stand for, every weapon it can carry (template inventory sources and
    /// leveled lists followed through all entries) and whether it can be drawn in its
    /// hands: model, not embedded, stance clips in its skeleton folder, a `Weapon` node
    /// (fnv.actor.creature_weapons/1).
    CreatureWeapons {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
    },
    /// Furniture (FURN): the entry points of each model (`BSFurnitureMarker`), marker
    /// flags, how many references place it, and the clips the IDLE tree names for
    /// getting on, staying on and getting off per marker ID (fnv.actor.furniture/1).
    Furniture {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Leave out the per-furniture list.
        #[arg(long)]
        summary_only: bool,
    },
    /// Locomotion clips of a skeleton folder (idle, walk, run, turn per body and stance)
    /// and the root motion of each: accumulation-root translation and turn per cycle,
    /// ground speed (fnv.actor.locomotion/1).
    Locomotion {
        /// The game's Data folder (archives are read from it).
        data_dir: PathBuf,
        /// NIF schema directory.
        #[arg(long)]
        nif_schema: PathBuf,
        /// Skeleton folder (VFS path).
        #[arg(long, default_value = r"meshes\characters\_male")]
        folder: String,
        /// Also sample the accumulation root and `Bip01 NonAccum` at this many points per
        /// clip.
        #[arg(long, default_value_t = 0)]
        samples: usize,
        /// Also every attack clip in the folder and below it (file name containing
        /// `attack`), for the travel of attack clips (actors.md §8.13).
        #[arg(long)]
        attacks: bool,
    },
    /// The FaceGen `.tri` files (morph targets of head parts): morph labels, and whether
    /// each has the vertices of the model beside it (fnv.actor.tri/1).
    Tri {
        /// The game's Data folder (archives are read from it).
        data_dir: PathBuf,
        /// NIF schema directory.
        #[arg(long)]
        nif_schema: PathBuf,
        /// One file (VFS path) in detail: each morph's scale and reach.
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PhysCmd {
    /// Build the static world around a cell and drop probe characters (fnv.phys.cell/1).
    Cell {
        #[command(flatten)]
        w: WorldArgs,
        /// Cell Editor ID or load-order FormID.
        #[arg(long)]
        cell: Option<String>,
        /// Worldspace Editor ID or FormID, with --x and --y.
        #[arg(long)]
        world: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
        /// Exterior cells loaded around the centre cell, in grid squares.
        #[arg(long, default_value_t = 1)]
        radius: i32,
        /// Drop point "x,y" or "x,y,z" in world units (repeatable; default: the centre
        /// of the cell, 100 units above the ground).
        #[arg(long = "drop", allow_hyphen_values = true)]
        drops: Vec<String>,
        /// Simulated seconds per probe.
        #[arg(long, default_value_t = 5.0)]
        seconds: f32,
        /// NIF schema directory for mesh collision (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Leave mesh collision out (terrain and collision primitives only).
        #[arg(long)]
        no_meshes: bool,
        /// Also list every reference with collision: base, layers, shells and what a
        /// vertical ray at its position hits first (`ref_list`).
        #[arg(long)]
        list_refs: bool,
        /// Labelled drops (JSON, `fnv.phys.drops/1`, physics.md §7): each becomes a probe,
        /// and `observations` lists every label with its final feet.
        #[arg(long)]
        drops_file: Option<PathBuf>,
        /// Ray "x,y,z,dx,dy,dz[,max]" (repeatable; max default 10000): the first hit of
        /// every collider (all layers) is listed under `rays`.
        #[arg(long = "ray", allow_hyphen_values = true)]
        rays: Vec<String>,
    },
    /// A scripted character walk through the world built around its start, traced step
    /// by step (fnv.phys.walk/1).
    Walk {
        #[command(flatten)]
        w: WorldArgs,
        /// Walk script (JSON, `fnv.phys.walk-script/1`, physics.md §7).
        #[arg(long)]
        script: PathBuf,
        /// Interior cell to walk in (Editor ID or FormID).
        #[arg(long)]
        cell: Option<String>,
        /// Worldspace to walk in (Editor ID or FormID); the cells around the start load.
        #[arg(long)]
        world: Option<String>,
        /// Exterior cells loaded around the start cell, in grid squares.
        #[arg(long, default_value_t = 1)]
        radius: i32,
        /// Record every Nth step (the last step is always recorded).
        #[arg(long, default_value_t = 1)]
        every: usize,
        /// NIF schema directory for mesh collision (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Leave mesh collision out (terrain and collision primitives only).
        #[arg(long)]
        no_meshes: bool,
    },
    /// Terrain height under fixed sample points of every landscape cell of a worldspace,
    /// found by vertical rays against the physics terrain (fnv.phys.heights/1).
    Heights {
        #[command(flatten)]
        w: WorldArgs,
        #[arg(long)]
        world: String,
    },
    /// Vertical rays on every landscape cell of a worldspace against the analytic
    /// terrain surface (fnv.phys.terrain/1).
    Terrain {
        #[command(flatten)]
        w: WorldArgs,
        #[arg(long)]
        world: String,
        /// Scattered sample points per cell (grid vertices and lines are added).
        #[arg(long, default_value_t = 64)]
        samples: usize,
    },
}

#[derive(Subcommand)]
pub enum CollideCmd {
    /// Statistics of every collision object and the measured stored-to-game scale
    /// (fnv.collide.survey/1).
    Survey {
        /// BSA archives, loose .nif files or directories.
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(long)]
        schema: PathBuf,
        /// Only files whose path contains this text (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
    },    /// Every solid collision object of every mesh in an archive, in game units
    /// (fnv.collide.objects/1).
    Objects {
        archive: PathBuf,
        #[arg(long)]
        schema: PathBuf,
        /// Only files whose path contains this text (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
    },
    /// The jointed bodies (ragdolls) of every mesh that has constraints: bodies, joints
    /// and how the bind pose sits in the joints' limits (fnv.collide.ragdoll/1).
    Ragdoll {
        /// BSA archives, loose .nif files or directories.
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(long)]
        schema: PathBuf,
        /// Only files whose path contains this text (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
        /// Leave out the per-file rows (statistics only).
        #[arg(long)]
        summary: bool,
    },
}

#[derive(Args)]
pub struct WorldArgs {
    #[command(flatten)]
    pub lo: LoadOrderArgs,
    #[arg(long)]
    pub schema: PathBuf,
}

#[derive(Args)]
pub struct GridArgs {
    /// Worldspace Editor ID or load-order FormID.
    #[arg(long)]
    pub world: String,
    #[arg(long, allow_hyphen_values = true)]
    pub x: i32,
    #[arg(long, allow_hyphen_values = true)]
    pub y: i32,
}

#[derive(Subcommand)]
pub enum WorldCmd {
    /// Every worldspace with cell and reference counts (fnv.world.list/1).
    List {
        #[command(flatten)]
        w: WorldArgs,
    },
    /// Cells of a worldspace, or all interior cells (fnv.world.cells/1).
    Cells {
        #[command(flatten)]
        w: WorldArgs,
        /// Worldspace Editor ID or FormID; omit for interior cells.
        #[arg(long)]
        world: Option<String>,
    },
    /// References placed in one cell, every cell of a worldspace, or every interior cell
    /// (fnv.world.refs/1).
    #[command(group(clap::ArgGroup::new("which").required(true).args(["cell", "world", "interiors"])))]
    Refs {
        #[command(flatten)]
        w: WorldArgs,
        /// Cell Editor ID or load-order FormID.
        #[arg(long)]
        cell: Option<String>,
        /// Every cell of this worldspace (its persistent cell included); with --x and
        /// --y only the exterior cell at that grid position.
        #[arg(long)]
        world: Option<String>,
        /// Every interior cell.
        #[arg(long)]
        interiors: bool,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
    },
    /// Landscape of one exterior cell (fnv.world.land/1).
    Land {
        #[command(flatten)]
        w: WorldArgs,
        #[command(flatten)]
        g: GridArgs,
        /// Also normals, vertex colours and texture layers.
        #[arg(long)]
        full: bool,
    },
    /// Height summary of every landscape cell in a worldspace (fnv.world.heights/1).
    Heights {
        #[command(flatten)]
        w: WorldArgs,
        #[arg(long)]
        world: String,
    },
    /// Observational checks of the world conventions (fnv.world.check/1).
    Check {
        #[command(flatten)]
        w: WorldArgs,
        /// Worldspace to check (default: every worldspace).
        #[arg(long)]
        world: Option<String>,
    },
    /// Music and placed sounds (fnv.world.audio/1): music types, media sets and location
    /// controllers, acoustic spaces; with --cell the audio markers and sound markers in
    /// earshot of it, and with --at what is heard there.
    Audio {
        #[command(flatten)]
        w: WorldArgs,
        /// Cell Editor ID or load-order FormID (an exterior cell stands for its worldspace).
        #[arg(long)]
        cell: Option<String>,
        /// Listener position "x,y,z" in world units (with --cell).
        #[arg(long, allow_hyphen_values = true, requires = "cell")]
        at: Option<String>,
        /// Game hour for the day or night choice (with --at).
        #[arg(long, default_value_t = 12.0)]
        hour: f32,
    },
    /// Radios (fnv.world.radio/1): radio stations (talking activators), their placed
    /// references and broadcast range, the receivers (activators naming a station) and
    /// the lines their quests give them (docs/spec-notes/radio.md).
    Radio {
        #[command(flatten)]
        w: WorldArgs,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
    },
    /// Records' alternate textures (fnv.world.alt_textures/1): per MODS/MO2S/MO3S/MO4S
    /// entry the texture set, the model shapes it goes to and whether its files exist
    /// (docs/spec-notes/texture-swaps.md).
    AltTextures {
        #[command(flatten)]
        w: WorldArgs,
        /// NIF schema directory (default: `<schema>/../nif`).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// List only these records (Editor IDs); the summary counts every record.
        #[arg(long)]
        edid: Vec<String>,
    },
    /// Terminals (fnv.world.terminals/1): hacking difficulty, flags, menu items, scripts,
    /// password notes and placed references (docs/spec-notes/hacking.md).
    Terminals {
        #[command(flatten)]
        w: WorldArgs,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
    },
    /// Every placed reference whose model is ragdoll-like or which carries XRGD, with the
    /// branch NVForever takes (fnv.world.posed_clutter/1; dead-on-spawn.md §5.2).
    PosedClutter {
        #[command(flatten)]
        w: WorldArgs,
        /// NIF schema directory (default: `<schema>/../nif`).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Leave out the per-reference rows (summary counts only).
        #[arg(long)]
        summary: bool,
    },
    /// One terminal hacking game (fnv.world.hack/1) from the game's word list: the dump
    /// as the screen shows it, the words and their likeness, and the log after `--pick`s.
    Hack {
        /// Data folder (its archives hold menusalloutdict.txt).
        data_dir: PathBuf,
        /// 0 Very Easy … 4 Very Hard, 5 Requires Key.
        #[arg(long, default_value_t = 2)]
        difficulty: u8,
        /// The player's Science skill.
        #[arg(long, default_value_t = 50.0)]
        science: f32,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Dump indices (column 1 rows 0-16 are 0-203, column 2 204-407) picked in order.
        #[arg(long, value_delimiter = ',')]
        pick: Vec<usize>,
    },
}

#[derive(Subcommand)]
pub enum SimCmd {
    /// What the simulation loaders understood, with consistency checks (fnv.sim.forms/1).
    Forms {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
    },
    /// Every inventory item base form with value, weight, health and body slots
    /// (fnv.sim.items/1).
    Items {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
    },
    /// Every container base form with its authored contents (fnv.sim.containers/1).
    Containers {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
    },
    /// Leveled-list outcome histograms for the cases in a fnv.sim.cases/1 file
    /// (fnv.sim.samples/1).
    Sample {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        cases: PathBuf,
    },
    /// Run a scripted action sequence (fnv.sim.actions/1) and print its trace
    /// (fnv.sim.trace/1). An actor moved by a teleport comes to rest by the physics
    /// arrival rule in the destination's collision world (simulation.md §7).
    Run {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        actions: PathBuf,
        /// NIF schema directory for mesh collision (default: `nif` next to --schema).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Keep teleported actors at the stored destination (no physics placement).
        #[arg(long)]
        no_physics: bool,
    },
    /// Every teleport door by where it leads (interior, same worldspace, another
    /// worldspace), with the doors into another worldspace listed (fnv.sim.doors/1).
    Doors {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
    },
    /// Every DOOR base: flags, sounds, placed references (teleport or not, locked) and
    /// the animation sequences and collision bodies of its model (fnv.sim.door_models/1).
    DoorModels {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `<schema>/../nif`).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
    },
    /// Every activator and talking activator base (radio receivers apart): the controller
    /// sequences of its model and what their tracks drive (fnv.sim.activator_models/1).
    ActivatorModels {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `<schema>/../nif`).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
    },
    /// Scripts' PlayGroup and IsAnimPlaying calls: on which objects, with which groups and
    /// flags, and whether the object's model has that sequence (fnv.sim.playgroup_calls/1).
    PlaygroupCalls {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        /// NIF schema directory (default: `<schema>/../nif`).
        #[arg(long)]
        nif_schema: Option<PathBuf>,
        /// Function catalogue (default: `<schema>/../script/functions.json`).
        #[arg(long)]
        functions: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum SptCmd {
    /// Parse every .spt in an archive and generate each tree (fnv.spt.listing/1).
    List { archive: PathBuf },
    /// Every entry of one .spt, from an archive (--path) or a loose file (fnv.spt.dump/1).
    Dump {
        source: PathBuf,
        #[arg(long)]
        path: Option<String>,
    },
    /// Level parameters and generated width : height of each .spt under several readings
    /// and measuring rules (docs/spec-notes/speedtree.md §7; fnv.spt.shape/1).
    Shape {
        archive: PathBuf,
        /// Only paths containing this text (e.g. `euonymus`).
        #[arg(long)]
        filter: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum DdsCmd {
    /// Header, format and mip layout of every .dds in an archive (fnv.dds.listing/1).
    List { archive: PathBuf },
}

#[derive(Args)]
pub struct HomeArg {
    /// NVForever home directory (default: NVFOREVER_HOME, else Documents\My Games\NVForever
    /// on Windows, the platform data dir elsewhere).
    #[arg(long)]
    pub home: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum BsaCmd {
    /// List every file with hashes, sizes and (by default) a content SHA-256.
    List {
        archive: PathBuf,
        /// Skip decoding files (no size/sha256 columns).
        #[arg(long)]
        no_content_hash: bool,
    },
    /// Extract one file.
    Extract {
        archive: PathBuf,
        /// Path inside the archive (case-insensitive, / or \).
        path_in_archive: String,
        out_file: PathBuf,
    },
    /// Byte-identical rewrite check plus decode of every file.
    Verify { archive: PathBuf },
    /// The game's start-up check (headers and tables only, nothing decompressed); fails
    /// when any archive fails it.
    Check { archives: Vec<PathBuf> },
}

#[derive(Subcommand)]
pub enum VfsCmd {
    /// The archives mounted for a load order, lowest precedence first (loose files go
    /// above them all), as the game resolves them (fnv.vfs.order/1).
    Order {
        /// The game's Data folder.
        data_dir: PathBuf,
        /// Plugins in load order (default: the base game and its add-on masters).
        plugins: Vec<String>,
        /// A base archive list in place of the shipped `SArchiveList` (comma-separated,
        /// lowest precedence first), as NVForever.ini's `[Archive] SArchiveList` sets it.
        #[arg(long)]
        archive_list: Option<String>,
    },
    /// A Mod Organizer 2 instance's profile as the merge it means (#1526): the enabled
    /// mods from the lowest priority up, the overwrite folder last, and the profile's
    /// active plugins (fnv.vfs.mods/1). With `--data` and `--path`, the path is resolved
    /// through those layers as the engine reads it.
    Mods {
        /// The instance folder (the one holding ModOrganizer.ini).
        instance: PathBuf,
        /// Profile name (default: the instance's selected_profile).
        #[arg(long)]
        profile: Option<String>,
        /// The game's Data folder, for `--path`.
        #[arg(long)]
        data: Option<PathBuf>,
        /// A game path to resolve (e.g. `textures\clutter\vodka.dds`).
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum EsmCmd {
    /// TES4 header.
    Header { plugin: PathBuf },
    /// Record/group counts.
    Stats { plugin: PathBuf },
    /// Every record with FormID, type, EDID and FULL.
    Inventory { plugin: PathBuf },
    /// Byte-identical rewrite and per-record payload checks.
    Verify { plugin: PathBuf },
    /// Schema coverage report (work queue of undecoded subrecords), over one plugin or
    /// several together.
    Coverage {
        #[arg(required = true, num_args = 1..)]
        plugins: Vec<PathBuf>,
        #[arg(long)]
        schema: PathBuf,
        #[arg(long, default_value_t = 200)]
        max_rows: usize,
    },
    /// Write a plugin moving placed references (the editor's writer, fnv_esm::edit): each
    /// of `refs` (load-order FormIDs in hex) by `dx` units along X, after `plugins`.
    EditMove {
        #[arg(required = true, num_args = 1..)]
        plugins: Vec<PathBuf>,
        #[arg(long)]
        schema: PathBuf,
        #[arg(long, required = true, num_args = 1.., value_parser = parse_formid)]
        refs: Vec<u32>,
        #[arg(long, default_value_t = 100.0)]
        dx: f32,
        #[arg(long)]
        out: PathBuf,
    },
    /// Decode every subrecord with the schema, encode it back and compare the bytes
    /// (the editor's encoder, fnv_esm::encode); one report over all the plugins.
    EncodeCheck {
        #[arg(required = true, num_args = 1..)]
        plugins: Vec<PathBuf>,
        #[arg(long)]
        schema: PathBuf,
        #[arg(long, default_value_t = 200)]
        max_rows: usize,
    },
    /// Decode records with the schema, selected by FormID or Editor ID.
    #[command(group(clap::ArgGroup::new("select").required(true).args(["formid", "edid"])))]
    Decode {
        plugin: PathBuf,
        #[arg(long)]
        schema: PathBuf,
        /// File-local FormID in hex, with or without 0x.
        #[arg(long, value_parser = parse_formid)]
        formid: Option<u32>,
        /// Editor ID (case-insensitive).
        #[arg(long)]
        edid: Option<String>,
    },
    /// Load-order resolution report.
    Loadorder {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Also resolve every FormID-typed field the schema knows about.
        #[arg(long)]
        schema: Option<PathBuf>,
    },
    /// Observational statistics per (record type, subrecord) for schema work.
    Survey {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Only survey this plugin (case-insensitive name).
        #[arg(long)]
        plugin: Option<String>,
    },
    /// Every schema field and flag bit the records of a load order carry: per record
    /// type, subrecord and field path, how often present, how often non-zero, the most
    /// common values and each flag bit's count (fnv.esm.fields/1).
    Fields {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum NifCmd {
    /// Schema coverage over many files: per block type, how many decode exactly and
    /// re-encode byte-identically (the Layer 2 work queue).
    Coverage {
        /// BSA archives, loose .nif/.kf files or directories.
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(long)]
        schema: PathBuf,
        /// Only files whose path contains this text (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, default_value_t = 200)]
        max_rows: usize,
        /// Skip the re-encode check (faster).
        #[arg(long)]
        no_reencode: bool,
    },
    /// Per-file header summary of every .nif/.kf in an archive (fnv.nif.stats/1).
    Stats { archive: PathBuf },
    /// Scene objects and geometry summaries of every .nif/.kf in an archive (fnv.nif.scene/1).
    Scene {
        archive: PathBuf,
        #[arg(long)]
        schema: PathBuf,
    },
    /// Animation sequences of every .kf/.nif in an archive, with sanity statistics
    /// for the evaluated tracks (fnv.nif.anim/1).
    Anim {
        archive: PathBuf,
        #[arg(long)]
        schema: PathBuf,
        /// Only files whose path contains this text.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Every texture path referenced by every mesh in the virtual file system (default
    /// archive order + loose files), and whether it resolves (fnv.nif.textures/1).
    Textures {
        data_dir: PathBuf,
        #[arg(long)]
        schema: PathBuf,
        #[arg(long, default_value_t = 200)]
        max_rows: usize,
    },
    /// Every distinct object name, extra-data name/value and block type in the archives,
    /// counted by files, for finding nodes the engine never reads (fnv.nif.nodes/1).
    Nodes {
        /// BSA archives, loose .nif/.kf files or directories.
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(long)]
        schema: PathBuf,
        /// Example file paths kept per entry.
        #[arg(long, default_value_t = 3)]
        examples: usize,
        /// Most common values kept per extra-data name.
        #[arg(long, default_value_t = 20)]
        values: usize,
    },
    /// Decode one file's blocks with the schema.
    Decode {
        /// A loose file, or a BSA archive together with --path.
        source: PathBuf,
        /// Path inside the archive.
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        schema: PathBuf,
        /// Only this block index.
        #[arg(long)]
        block: Option<usize>,
    },
}

#[derive(Args)]
pub struct LoadOrderArgs {
    pub data_dir: PathBuf,
    /// Plugins in load order: names in the data folder, or full paths. `--mo2-profile
    /// <profile folder>` (anywhere in the arguments) appends a Mod Organizer 2 profile's
    /// active plugins as full paths instead.
    #[arg(required = true)]
    pub plugins: Vec<String>,
}

pub(crate) fn parse_formid(s: &str) -> Result<u32, String> {
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u32::from_str_radix(hex, 16).map_err(|_| format!("not a hex FormID: {s}"))
}

pub(crate) fn emit(v: &Value) {
    use std::io::Write;
    let mut lock = std::io::stdout().lock();
    let _ = lock.write_all(v.to_string().as_bytes());
    let _ = lock.write_all(b"\n");
}

/// Diagnostics go to stderr. Default level is `warn`; set `NVFOREVER_LOG` (an
/// `EnvFilter` directive such as `debug` or `fnv_esm=trace`) for more.
fn init_logging() {
    use std::io::IsTerminal;
    let filter = tracing_subscriber::EnvFilter::try_from_env("NVFOREVER_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}

fn main() -> ExitCode {
    crash_report::install();
    // `--mo2-profile <dir>` becomes the profile's plugins as full paths (mo2_args).
    let args = match mo2_args::expand(std::env::args_os().collect()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let cli = Cli::parse_from(args);
    init_logging();
    // The code page of the game's text: NVF_CODE_PAGE, else Windows-1252 (#1439).
    match fnv_util::text::chosen(None) {
        Ok(c) => fnv_util::text::set_code_page(c),
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    }
    // The game folder a command reads (its data-folder argument, wherever the command
    // takes it): every `fnv_install::guard_write` then refuses outputs inside it (#994).
    if let Some(a) = std::env::args_os().skip(1).map(PathBuf::from).find(|p| p.is_dir() && fnv_install::GameInstall::open(p).is_ok()) {
        fnv_install::remember_game_at(&a);
    }
    let res = match cli.cmd {
        Cmd::Setup { game, home } => install_cmd::setup(&game, &home),
        Cmd::Paths { home } => install_cmd::paths(&home),
        Cmd::Bsa(c) => bsa_cmd::run(c),
        Cmd::Vfs(c) => bsa_cmd::run_vfs(c),
        Cmd::Esm(c) => esm_cmd::run(c),
        Cmd::Nif(c) => nif_cmd::run(c),
        Cmd::Dds(c) => dds_cmd::run(c),
        // `pbr check` and `pbr selftest` print their report and may still fail.
        Cmd::Pbr(c) => return pbr_cmd::main(c),
        Cmd::Spt(c) => spt_cmd::run(c),
        Cmd::World(c) => world_cmd::run(c),
        Cmd::Phys(c) => phys_cmd::run(c),
        Cmd::Collide(c) => collide_cmd::run(c),
        Cmd::Sim(c) => sim_cmd::run(c),
        Cmd::Script(c) => script_cmd::run(c),
        Cmd::Dialogue(c) => dialogue_cmd::run(c),
        Cmd::Quest(c) => quest_sweep::run(c),
        Cmd::Coop(c) => coop_cmd::run(c),
        Cmd::Actor(c) => actor_cmd::run(c),
        Cmd::Combat(c) => combat_cmd::run(c),
        Cmd::Nav(c) => nav_cmd::run(c),
        Cmd::Ai(c) => ai_cmd::run(c),
        Cmd::Voice(c) => voice_cmd::run(c),
        Cmd::Ext(c) => ext_cmd::run(c),
        Cmd::Ui(c) => ui_cmd::run(c),
        Cmd::Save(c) => save_cmd::run(c),
    };
    match res {
        Ok(v) => {
            emit(&v);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn formid_forms() {
        assert_eq!(parse_formid("0x0001F4A2"), Ok(0x1F4A2));
        assert_eq!(parse_formid("0X1f4a2"), Ok(0x1F4A2));
        assert_eq!(parse_formid("1F4A2"), Ok(0x1F4A2));
        assert!(parse_formid("zz").is_err());
    }
}
