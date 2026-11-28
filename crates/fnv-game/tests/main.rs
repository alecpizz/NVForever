//! fnv-game's integration tests, built as one test binary (`autotests = false` in Cargo.toml).
//! One binary instead of one per file: the crate and its dependencies are linked and
//! optimised once, not once per file. Each file stays a module of its own; run one with
//! `cargo test -p fnv-game --release --test integration <file>::`.
//! The files that change process-wide state (mod_files' roots, nvse_jg7's combat-music
//! flag, source_compile's mode) are test binaries of their own, listed in Cargo.toml: in
//! one process they would change each other's, or the setting other tests read.
//! A new file goes in the list below or, if it sets such state, in Cargo.toml as a
//! `[[test]]`; `every_test_file_is_built` fails until it is in one of them.

mod common;
mod string_settings;
#[path = "common/digest.rs"]
mod digest;

mod abilities;
mod actor_base_flags;
mod actor_values;
mod aid;
mod ammo_overrides;
mod appearance;
mod area_effects;
mod armo_overrides;
mod ash_piles;
mod build_identity;
mod camera_paths;
mod caravan_data;
mod clone_calling_reference;
mod cloned_sound_traits;
mod combat_blocks;
mod combat_lines_data;
mod combat_transitions;
mod companion_wheel;
mod container_sound_getters;
mod crime;
mod critical_player_state;
mod csty_session;
mod crippled_limbs_data;
mod dead_on_spawn;
mod delayed_calls;
mod death_items;
mod destruction_stage_blocks;
mod dialogue_data;
mod dialogue;
mod coop_encounters;
mod coop_kills;
mod difficulty;
mod effect_updates;
mod experience;
mod explosion_overrides;
mod extra_camera;
mod player_source_commands;
mod pipboy_mode;
mod extender_conditions;
mod factions;
mod fixtures_data;
mod first_person_menu_clock;
mod friend_hit_condition;
mod functions;
mod game_clock;
mod title_menu_group;
mod jip_remove_filters;
mod key_button_events;
mod hit_effects_data;
mod hit_effects;
mod idles_holstered;
mod idles;
mod imagespace_weather_fog_gmst;
mod ini_display_size;
mod item_ref_health;
mod jg_nif;
mod jg10;
mod jg3;
mod jip_animaction_playgroup;
mod jip_combat_events;
mod jip_fast_travel_event;
mod jip_main_loop_zero;
mod jip_target_and_trigger_events;
mod jlm_dropped_refs;
mod jg4;
mod jg5;
mod jg6;
mod jg8;
mod jip10;
mod jip11;
mod jip12;
mod jip13;
mod jip14;
mod jip6;
mod jip7;
mod jip8;
mod jip9;
mod knockdowns;
mod lighting_session;
mod lutana;
mod nellis_hangar_doors;
mod nif_player_models;
mod model_has_block;
mod script_node_mods;
mod lutana2;
mod lutana3;
mod magic_archetypes;
mod man_on_fire;
mod npc_doors;
mod null_call_context;
mod nvse_itr2;
mod nvse_stubs_batch1;
mod nvse_stubs_batch3;
mod nvsescript;
mod one111;
mod package_choice;
mod packages;
mod perk_combat;
mod perk_talk;
mod pp;
mod projectile_trait_overrides;
mod quest_marker_data;
mod radiation;
mod radio_data;
mod respawn;
mod save_pairs;
mod script_events;
mod script_stack;
mod short_hex_receivers;
mod showoff3;
mod set_class;
mod showoff4;
mod showoff5;
mod showoff_combat_events;
mod showoff_on_add;
mod showoff_quest_imod_events;
mod softlock;
mod spawn_placeholders;
mod source_metadata;
mod spell_alch_mgef_overrides;
mod step_budget;
mod sup1;
mod sup4;
mod sup6;
mod sup7;
mod sweep;
mod temporary_scene_bases;
mod traps;
mod ui_functions;
mod ui_hud;
mod ui_loading;
mod ui_mcm;
mod ui_more;
mod ui_ysi;
mod ysi_expression_text_shift;
mod unconscious;
mod unlisted;
mod vendor_services;
mod voice_gate;
mod weapon_sound_overrides;
mod zones;

/// With `autotests = false` Cargo builds only the files named, so a new file in `tests/`
/// would never run: every one is a `mod` here or a `[[test]]` in Cargo.toml, not both.
mod world_model_fields;

#[test]
fn every_test_file_is_built() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let main = std::fs::read_to_string(dir.join("tests/main.rs")).unwrap();
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let mods: Vec<&str> = main.lines().filter_map(|l| l.strip_prefix("mod ")?.strip_suffix(';')).collect();
    let own: Vec<&str> =
        manifest.lines().filter_map(|l| l.trim().strip_prefix("path = \"tests/")?.strip_suffix(".rs\"")).collect();
    let mut problems = Vec::new();
    for entry in std::fs::read_dir(dir.join("tests")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if stem == "main" {
            continue;
        }
        match (mods.contains(&stem.as_str()), own.contains(&stem.as_str())) {
            (false, false) => problems.push(format!("tests/{stem}.rs is not built: add `mod {stem};` to tests/main.rs")),
            (true, true) => problems.push(format!("tests/{stem}.rs is built twice: in tests/main.rs and as a [[test]]")),
            _ => {}
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

mod callable_identity;
mod default_open;
mod em_clearance;
mod followers_and_detecting_actors;
mod form_override_queries;
mod fos_convert;
mod improved_console;
mod is_in_air;
mod is_in_kill_cam;
mod no_unequip;
mod nvse_stubs_batch2;
mod pc_toddler_young;
mod preactivation_events;
mod preactivation_mutation;
mod stewie_945_960_970_qualification;
mod worldspace_climate_imagespace;

// The co-op shared-progress test files.
mod coop_guest_doctor_data;
mod coop_host_story;
mod coop_joint;
mod coop_quest_items_data;
mod coop_story_gate;
mod coop_uniques;
mod story_guards;
