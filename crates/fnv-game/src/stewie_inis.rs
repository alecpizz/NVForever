//! Stewie Tweaks modular override folder loading (`docs/spec-notes/stewie-modular-inis.md`).
//!
//! Loads standalone `.ini` override files from:
//! - `Data\NVSE\Plugins\Tweaks\INIs\*.ini` (modular tweak & option overrides)
//! - `Data\NVSE\Plugins\Tweaks\GameSettings\*.ini` (modular GMST overrides)
//!
//! [documented: Stewie's Tweaks public Nexus Mods documentation/readme
//! (`https://www.nexusmods.com/newvegas/mods/66347`) and GECK wiki `StewieGetValue` —
//! no Stewie Tweaks DLL or implementation source code was consulted.]
//!
//! When neither modular folder exists or contains `.ini` files, vanilla and single-INI
//! (`nvse_stewie_tweaks.ini`) behavior is unchanged.

use crate::{mod_files::ModFiles, tweaks::Tweaks};
use fnv_sim::{settings::Setting, FormStore, World};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Base Stewie Tweaks INI relative to `Data`.
pub const BASE_INI_PATH: &str = "NVSE\\Plugins\\nvse_stewie_tweaks.ini";

/// Modular Stewie Tweaks override INI folder relative to `Data`.
pub const MODULAR_INIS_DIR: &str = "NVSE\\Plugins\\Tweaks\\INIs";

/// Modular Stewie GameSettings override INI folder relative to `Data`.
pub const GAME_SETTINGS_DIR: &str = "NVSE\\Plugins\\Tweaks\\GameSettings";

/// Merged Stewie INI entries keyed by lower-case `(section, key)` [design].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StewieIniMap {
    entries: BTreeMap<(String, String), String>,
}

/// Strip a leading UTF-8 BOM (either `\u{feff}` or latin-1 `\u{ef}\u{bb}\u{bf}`).
fn strip_bom_str(text: &str) -> &str {
    text.strip_prefix('\u{feff}')
        .or_else(|| text.strip_prefix("\u{ef}\u{bb}\u{bf}"))
        .unwrap_or(text)
}

/// Strip an inline `;` comment outside double quotes, then trim surrounding whitespace.
fn strip_inline_comment(raw: &str) -> &str {
    let mut in_quotes = false;
    for (idx, ch) in raw.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => return raw[..idx].trim(),
            _ => {}
        }
    }
    raw.trim()
}

/// Parse a section heading `[Section]`, ignoring inline comments after `]`.
fn parse_section_header(line: &str) -> Option<String> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    let name = rest[..end].trim();
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// Parse a Stewie INI boolean value (`1`/`0`, `true`/`false`, `yes`/`no`, `on`/`off`,
/// or any finite number where non-zero is `true`) [design].
pub fn parse_ini_bool(raw: &str) -> Option<bool> {
    let s = raw.trim().trim_matches('"').trim();
    if s.is_empty() {
        return None;
    }
    match s.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => s.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v != 0.0),
    }
}

/// Parse a Stewie INI integer value (integer literal or bounded finite float truncated to
/// `i32`) [design].
pub fn parse_ini_i32(raw: &str) -> Option<i32> {
    let s = raw.trim().trim_matches('"').trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(i) = s.parse::<i32>() {
        return Some(i);
    }
    let f = s.parse::<f64>().ok().filter(|v| v.is_finite())?.trunc();
    if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&f) {
        Some(f as i32)
    } else {
        None
    }
}

/// Parse a Stewie INI finite floating-point value [design].
pub fn parse_ini_f64(raw: &str) -> Option<f64> {
    let s = raw.trim().trim_matches('"').trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Unwrap one pair of surrounding double quotes if present.
fn unwrap_quotes(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}

impl StewieIniMap {
    /// Merge INI text into this map; later entries override earlier entries for the same
    /// case-insensitive `(section, key)` [design].
    pub fn parse_into(&mut self, text: &str) {
        let mut section = String::new();
        for line in strip_bom_str(text).lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') {
                if let Some(sec) = parse_section_header(trimmed) {
                    section = sec;
                }
                continue;
            }
            let Some((raw_key, raw_val)) = trimmed.split_once('=') else {
                continue;
            };
            let key = raw_key.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            let val = strip_inline_comment(raw_val).to_string();
            self.entries.insert((section.clone(), key), val);
        }
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.entries
            .get(&(section.trim().to_ascii_lowercase(), key.trim().to_ascii_lowercase()))
            .map(String::as_str)
    }

    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        self.get(section, key).and_then(parse_ini_bool)
    }

    pub fn get_i32(&self, section: &str, key: &str) -> Option<i32> {
        self.get(section, key).and_then(parse_ini_i32)
    }

    pub fn get_f64(&self, section: &str, key: &str) -> Option<f64> {
        self.get(section, key).and_then(parse_ini_f64)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Load and merge `Data\NVSE\Plugins\Tweaks\INIs\*.ini` from explicit `roots` in
/// case-insensitive filename order (`A..Z`) [design]. Returns `(file_count, map)`.
pub fn load_modular_inis_from_roots(roots: &[PathBuf]) -> (usize, StewieIniMap) {
    let files = crate::mod_files::read_folder_inis_from_roots(roots, MODULAR_INIS_DIR);
    let count = files.len();
    let mut map = StewieIniMap::default();
    for (_, text) in files {
        map.parse_into(&text);
    }
    (count, map)
}

/// Load and merge `Data\NVSE\Plugins\Tweaks\INIs\*.ini` from the current `mod_files`
/// read roots in case-insensitive filename order (`A..Z`) [design].
pub fn load_modular_inis_from_data() -> (usize, StewieIniMap) {
    let files = crate::mod_files::read_data_folder_inis(MODULAR_INIS_DIR);
    let count = files.len();
    let mut map = StewieIniMap::default();
    for (_, text) in files {
        map.parse_into(&text);
    }
    (count, map)
}

/// Load and merge `Data\NVSE\Plugins\Tweaks\INIs\*.ini` from both read roots and
/// `mod_files` session writes in case-insensitive filename order (`A..Z`) [design].
pub fn load_modular_inis(mod_files: &ModFiles) -> (usize, StewieIniMap) {
    let files = mod_files.read_folder_inis(MODULAR_INIS_DIR);
    let count = files.len();
    let mut map = StewieIniMap::default();
    for (_, text) in files {
        map.parse_into(&text);
    }
    (count, map)
}

/// Query a `key:section` setting across `nvse_stewie_tweaks.ini` layered beneath
/// `Data\NVSE\Plugins\Tweaks\INIs\*.ini` (sorted `A..Z`, modular files overriding the
/// base INI) [design]. When no modular `.ini` files exist, reads only the base INI.
/// Does NOT trim `key` or `section` (matching observed `StewieGetValue`/`StewVal`
/// whitespace semantics in `w4h_08_stewie_get_value_whitespace`).
pub fn query_value(mod_files: &ModFiles, key: &str, section: &str) -> Option<String> {
    let mut merged = StewieIniMap::default();
    if let Some(base) = mod_files.read_text_any_case(BASE_INI_PATH) {
        merged.parse_into(&base);
    }
    for (_, text) in mod_files.read_folder_inis(MODULAR_INIS_DIR) {
        merged.parse_into(&text);
    }
    merged.entries.get(&(section.to_ascii_lowercase(), key.to_ascii_lowercase())).cloned()
}

/// Apply recognized modular tweak overrides from `Data\NVSE\Plugins\Tweaks\INIs\*.ini`
/// onto `fnv_game::tweaks::Tweaks` [design]. Returns how many fields were updated.
///
/// Only modular folder overrides should be passed here; the base
/// `nvse_stewie_tweaks.ini` does not supply defaults for NVForever-owned settings
/// (`docs/spec-notes/stewie-stat-profile.md`).
pub fn apply_modular_tweaks_to_game(overrides: &StewieIniMap, tweaks: &mut Tweaks) -> usize {
    if overrides.is_empty() {
        return 0;
    }
    let mut applied = 0usize;
    macro_rules! bool_tweak {
        ($section:expr, $key:expr, $field:ident) => {
            if let Some(v) = overrides.get_bool($section, $key) {
                tweaks.$field = v;
                applied += 1;
            }
        };
    }

    // [Tweaks] boolean switches documented in `crates/fnv-game/src/tweaks.rs` and
    // `docs/design/tweaks.md`.
    bool_tweak!("Tweaks", "bPowerArmorNeedsNoTraining", power_armor_needs_no_training);
    bool_tweak!("Tweaks", "bSleepInOwnedBeds", sleep_in_owned_beds);
    bool_tweak!("Tweaks", "bRemoveChemWarnOffIMOD", no_chem_wore_off_effect);
    bool_tweak!("Tweaks", "bNoSkillMessageIfIncreaseIsZero", no_book_message_without_gain);
    bool_tweak!("Tweaks", "bCapitalizeRecipeCategories", capitalize_recipe_categories);
    bool_tweak!("Tweaks", "bShowWeaponAmmoUseInMenus", show_weapon_ammo_use);
    bool_tweak!("Tweaks", "bShowBookEffects", show_book_effects);
    bool_tweak!("Tweaks", "bSkipSkillMenuIfNoPointsToAssign", skip_skill_page_without_points);
    bool_tweak!("Tweaks", "bFasterHackingTransition", faster_hacking_transition);
    bool_tweak!("Tweaks", "bMapRemembersPosition", remember_map_position);
    bool_tweak!("Tweaks", "bPickLocksWithoutSkill", pick_locks_without_skill);
    bool_tweak!("Tweaks", "bHackTerminalsWithoutSkill", hack_terminals_without_skill);
    bool_tweak!("Tweaks", "bNoLockFailedTerminals", no_terminal_lockout);
    bool_tweak!("Tweaks", "bNoCasinoBans", no_casino_bans);
    bool_tweak!("Tweaks", "bLootUnconsciousVictims", loot_unconscious);
    bool_tweak!("Tweaks", "bReviveUnconsciousCompanions", revive_companions);
    bool_tweak!("Tweaks", "bPickpocketWornItems", pickpocket_worn_items);
    bool_tweak!("Tweaks", "bNoSneakAttacksCriticals", no_sneak_criticals);
    bool_tweak!("Tweaks", "bDisableReloadingNonEmptyClip", reload_only_when_empty);
    bool_tweak!("Tweaks", "bWeightlessWornArmor", weightless_worn_armor);
    bool_tweak!("Tweaks", "bWeightlessWornPowerArmor", weightless_worn_power_armor);
    bool_tweak!("Tweaks", "bHardcoreTweaks", hardcore_tweaks);
    bool_tweak!("Tweaks", "bEquipBrokenItems", equip_broken_items);
    bool_tweak!("Tweaks", "bKeepBrokenItemsEquipped", keep_broken_equipped);
    bool_tweak!("Tweaks", "bHostilesPrioritizePlayer", hostiles_prefer_player);
    bool_tweak!("Tweaks", "bInteriorExteriorMapDoorIcon", map_door_kinds);
    bool_tweak!("Tweaks", "bNoMinCompanionMapDistance", no_min_companion_distance);
    bool_tweak!("Tweaks", "bMapShowUnconsciousCompanions", map_unconscious_companions);
    bool_tweak!("Tweaks", "bCompanionsUseLocationFontOnMap", companions_location_font);
    bool_tweak!("Tweaks", "bMapSelectableCompanions", map_summon_companions);
    bool_tweak!("Tweaks", "bNoPoisonConfirm", no_poison_confirm);
    bool_tweak!("Tweaks", "bShowWeaponPoisonEffects", show_poison_effects);
    bool_tweak!("Tweaks", "bOnlyAllowWaitWhileSitting", wait_only_seated);
    bool_tweak!("Tweaks", "bNoUnconciousMessage", no_unconscious_message);
    bool_tweak!("Tweaks", "bNoAudioDistortion", no_voice_filter);
    bool_tweak!("Tweaks", "bNoSkipDialog", no_skip_dialogue);
    bool_tweak!("Tweaks", "bLockpickAllowMouse", lockpick_mouse_turn);
    bool_tweak!("Tweaks", "bNoMaxCasinoBet", no_max_casino_bet);
    bool_tweak!("Tweaks", "bDontShowContainerAfterLockpick", no_container_after_pick);
    bool_tweak!("Tweaks", "bDontOpenDoorsAfterLockpick", no_door_after_pick);
    bool_tweak!("Tweaks", "bPatchUnseenCellName", mark_unvisited_doors);
    bool_tweak!("Tweaks", "bNoExitHacking", no_exit_hacking);
    bool_tweak!("Tweaks", "bNoGrabOwnedItems", no_grab_owned);
    bool_tweak!("Tweaks", "bAutoUnlockTerminals", auto_unlock_terminals);
    bool_tweak!("Tweaks", "bHittingWeaponsDoesntDamageThem", weapons_unhurt_by_hits);
    bool_tweak!("Tweaks", "bGodModePreventsJamAnims", god_mode_no_jams);
    bool_tweak!("Tweaks", "bExplosionKnockdownAvoidanceUsesStrength", knockdown_by_strength);
    bool_tweak!("Tweaks", "bDrowningDrainsAP", drowning_drains_ap);
    bool_tweak!("Tweaks", "bAllowAidAtMaxHealth", aid_at_full_health);
    bool_tweak!("Tweaks", "bGrabbingItemsIsCrime", grabbing_is_theft);
    bool_tweak!("Tweaks", "bLockNeedsKeyShowName", lock_needs_key_name);
    bool_tweak!("Tweaks", "bNoSkillTags", no_skill_tags);
    bool_tweak!("Tweaks", "bPreventNPCAddiction", no_npc_addiction);
    bool_tweak!("Tweaks", "bPickpocketFleeingActors", pickpocket_fleeing);
    bool_tweak!("Tweaks", "bReloadingWithFullClipSwitchesAmmoType", full_reload_swaps_ammo);
    bool_tweak!("Tweaks", "bPreventStealingCapsAfterRepair", repair_caps_to_container);
    bool_tweak!("Tweaks", "bNoDamageMeleeWeaponIfTargetDead", no_wear_on_dead);
    bool_tweak!("Tweaks", "bLightStepAffectsCompanions", light_step_companions);
    bool_tweak!("Tweaks", "bReloadJamsAffectedByAgility", jam_at_reload_pace);
    bool_tweak!("Tweaks", "bSleepOnChairs", sleep_on_chairs);
    bool_tweak!("Tweaks", "bStealingSendsNoAlarm", stealing_no_alarm);
    bool_tweak!("Tweaks", "bNoCompanionKillXP", no_companion_kill_xp);
    bool_tweak!("Tweaks", "bNoDisarmCompanions", no_disarm_companions);
    bool_tweak!("Tweaks", "bNoSelfExplosionDamage", no_self_explosion_damage);
    bool_tweak!("Tweaks", "bSneakCriticalsOnlyMeleeWeapons", sneak_criticals_melee_only);
    bool_tweak!("Tweaks", "bAllowTeammatesUseMeltdown", teammates_meltdown);
    bool_tweak!("Tweaks", "bSpeedMultScalesJumpHeight", speed_scales_jump);
    bool_tweak!("Tweaks", "bDisableTutorialMessages", no_tutorials);
    bool_tweak!("Tweaks", "bNoKnockdownInGodmode", no_knockdown_in_god_mode);
    bool_tweak!("Tweaks", "bLockpickHackingMessageShowsCurrentSkill", show_current_skill);
    bool_tweak!("Tweaks", "bUnspokenNPCIndicator", mark_unspoken);
    bool_tweak!("Tweaks", "bHideCapsAddedMessages", no_caps_added_message);
    bool_tweak!("Tweaks", "bSubtitlesShowActorNames", subtitle_names);
    bool_tweak!("Tweaks", "bDialogueKeepVoiceActingNotes", keep_acting_notes);
    bool_tweak!("Tweaks", "bHideCompletedQuests", hide_completed_quests);
    bool_tweak!("Tweaks", "bHideMapMarkerFactionReputation", hide_marker_reputation);
    bool_tweak!("Tweaks", "bDisableShowQuestLocation", no_show_quest_location);
    bool_tweak!("Tweaks", "bHideRanksInTraitMenu", hide_trait_ranks);
    bool_tweak!("Tweaks", "bHideQuestItemWeightAndValue", hide_quest_item_figures);
    bool_tweak!("Tweaks", "bIndividialItemStats", single_item_figures);
    bool_tweak!("Tweaks", "bShowAlternateAmmoTypesAvailableInMenus", mark_other_ammo);
    bool_tweak!("Tweaks", "bReputationShowsFameInfamy", show_fame_infamy);
    bool_tweak!("Tweaks", "bShowQuestAndNoDRDTItemsInRepairMenu", repair_any_armor);
    bool_tweak!("Tweaks", "bSortMiscStats", sort_misc_stats);
    bool_tweak!("Tweaks", "bStatsMenuShowEffectTimeRemaining", effect_time_left);
    bool_tweak!("Tweaks", "bShowModNameInPerkDescriptions", perk_plugin_names);
    bool_tweak!("Tweaks", "b12HourPipboyClock", pipboy_12_hour);
    bool_tweak!("Tweaks", "bShowActiveQuestNotesShowsAllStartedQuests", notes_of_all_quests);
    bool_tweak!("Tweaks", "bAllowPickpocketIfAlreadyCaught", pickpocket_after_caught);
    bool_tweak!("Tweaks", "bSleepWaitAnywhere", wait_anywhere);
    bool_tweak!("Tweaks", "bDetachedBeams", detached_beams);
    bool_tweak!("Tweaks", "bDisableGodmodeOnLoad", god_mode_off_on_load);
    bool_tweak!("Tweaks", "bDisableStealthEffectInPipboy", no_stealth_shimmer_in_pipboy);
    bool_tweak!("Tweaks", "bPreventTeammateFootstepSounds", quiet_teammate_steps);
    bool_tweak!("Tweaks", "bNoEnvironmentRadiationInFullPowerArmor", power_armor_blocks_radiation);
    bool_tweak!("Tweaks", "bSkipVeryEasyLocksAtMaxSkill", skip_easy_locks);
    bool_tweak!("Tweaks", "bDisarmingMinesPicksThemUp", pick_up_disarmed);
    bool_tweak!("Tweaks", "bNoScaleNpcDamageByCondition", npc_weapons_whole);
    bool_tweak!("Tweaks", "bLethalHitsIncreaseLimbChallenges", lethal_knockdowns_count);
    bool_tweak!("Tweaks", "bCripplingDoesntDisarm", crippling_never_disarms);
    bool_tweak!("Tweaks", "bAutoWeaponNoFiringDelay", no_spin_up);
    bool_tweak!("Tweaks", "bGodModeAllowsCraftingWithoutRequirements", god_mode_free_crafting);
    bool_tweak!("Tweaks", "bSneakCriticalsHeadshotsOnly", sneak_criticals_head_only);
    bool_tweak!("Tweaks", "bCompareWeaponStats", compare_weapons);
    bool_tweak!("Tweaks", "bScaleAshpileSize", scale_ash_piles);
    bool_tweak!("Tweaks", "bReloadWhileFiring", reload_while_firing);
    bool_tweak!("Tweaks", "bPerBulletLoopingReloads", per_bullet_reload);
    bool_tweak!("Tweaks", "bRepairScalesCraftingCondition", repair_scales_crafting);
    bool_tweak!("Tweaks", "bNoHealingInCombat", no_healing_in_combat);
    bool_tweak!("Tweaks", "bPickpocketCaughtCausesDetection", pickpocket_caught_detects);
    bool_tweak!("Tweaks", "bIgnoreCompanionsVATS", vats_ignores_companions);
    bool_tweak!("Tweaks", "bFrenzyGrantsPlayerXP", frenzy_kill_xp);
    bool_tweak!("Tweaks", "bFreeWildWasteland", free_wild_wasteland);
    bool_tweak!("Tweaks", "bStatsMenuPercentageXP", stats_xp_percent);
    bool_tweak!("Tweaks", "bInvertPipboyRepairMenuSorting", repair_low_first);
    bool_tweak!("Tweaks", "bUseWeaponRepairKitsInRepairMenu", repair_kits_in_list);
    bool_tweak!("Tweaks", "bRepairItemsPreview", repair_preview);
    bool_tweak!("Tweaks", "bQuantityMenuRespectsCompanionCarryCap", quantity_companion_cap);
    bool_tweak!("Tweaks", "bArmorPreventsBloodDecals", armor_blocks_blood);
    bool_tweak!("Tweaks", "bEmboldenTagSkills", embolden_tags);
    bool_tweak!("Tweaks", "bPickpocketOverhaul", pickpocket_overhaul);
    bool_tweak!("Tweaks", "bHoldToActivate", hold_activate);
    bool_tweak!("Tweaks", "bQuickUse", quick_use);
    bool_tweak!("Tweaks", "bVATSThrowablesUseVisibility", vats_throw_visibility);
    bool_tweak!("Tweaks", "bRemoveFollowerTopics", quiet_followers);
    bool_tweak!("Tweaks", "bCriticalHitMessagesIncludeLimbName", crit_message_limb);
    bool_tweak!("Tweaks", "bCustomPopupIcons", need_icons);
    bool_tweak!("Tweaks", "bPreventRepairIfNotAtWorkbench", repair_at_workbench);
    bool_tweak!("Tweaks", "bCharacterRadiation", character_radiation);
    bool_tweak!("Tweaks", "bReallocateSkillPointsOnLevelup", reallocate_skill_points);
    bool_tweak!("Tweaks", "bLocalMapRespawnedCellIndicator", local_map_reset_cells);
    bool_tweak!("Tweaks", "bHoldAndReleaseThrowables", hold_release_throws);
    bool_tweak!("Tweaks", "bNPCsCanDisarmPlayer", npcs_disarm_player);
    bool_tweak!("Tweaks", "bNPCsDropLiveGrenades", npcs_drop_live_grenades);
    bool_tweak!("Tweaks", "bWaterSourcesShowH2O", water_shows_h2o);
    bool_tweak!("Tweaks", "bDisableCharacterRespec", no_respec);
    bool_tweak!("Tweaks", "bWeaponImpactEffects", weapon_impacts);
    bool_tweak!("Tweaks", "bRobotCompanionsHealWithScrapMetal", robots_heal_with_scrap);
    bool_tweak!("Tweaks", "bNoSkillBooksAbove100", no_skill_books_at_max);
    bool_tweak!("Tweaks", "bPauseOnSaveLoad", pause_on_load);
    bool_tweak!("Tweaks", "bSwapAmmoWithWeaponHolstered", swap_ammo_holstered);
    bool_tweak!("Tweaks", "bNoAutoContinueDialog", no_auto_continue_dialogue);
    bool_tweak!("Tweaks", "bDontSetQuestWhenObjectivesAdded", no_quest_set_on_objective);
    bool_tweak!("Tweaks", "bDontSetQuestWhenCompletingObjectives", no_quest_set_on_complete);
    bool_tweak!("Tweaks", "bHideReadNotes", hide_read_notes);
    bool_tweak!("Tweaks", "bVATSStopBurstIfTargetDead", vats_burst_stops_on_kill);
    bool_tweak!("Tweaks", "bVATSDequeueWeaponShotsOnDisarm", vats_drops_disarmed_shots);
    bool_tweak!("Tweaks", "bTargetProjectilesInVATS", vats_target_projectiles);
    bool_tweak!("Tweaks", "bVATSTargetPlayerProjectiles", vats_target_own_projectiles);
    bool_tweak!("Tweaks", "bVATSIgnoreNonExplosiveProjectiles", vats_ignore_non_explosive);
    bool_tweak!("Tweaks", "bVATSAutoTargetHead", vats_auto_head);
    bool_tweak!("Tweaks", "bRepairRequiresWeaponSkill", repair_needs_weapon_skill);
    bool_tweak!("Tweaks", "bHideUnknownNPCNames", hide_unknown_names);
    bool_tweak!("Tweaks", "bShowUsePasswordOnTerminalsWithNote", terminal_password_prompt);
    bool_tweak!("Tweaks", "bLockPickMenuKeyboardMovement", lockpick_move_keys);
    bool_tweak!("Tweaks", "bMapRecenterHotkey", map_recentre_key);
    bool_tweak!("Tweaks", "bMapLocationDisplayDistance", map_marker_distance);
    bool_tweak!("Tweaks", "bBarterQuantityMenuShowsPrice", quantity_shows_price);
    bool_tweak!("Tweaks", "bContainerQuantityMenuShowsWeight", quantity_shows_weight);
    bool_tweak!("Tweaks", "bHideReputationTabIfEmpty", hide_empty_reputation);
    bool_tweak!("Tweaks", "bPreviewPerksOnLevelUp", preview_perks);
    bool_tweak!("Tweaks", "bMarkNotesUnread", mark_notes_unread);
    bool_tweak!("Tweaks", "bShiftIgnoresFriendlyVATS", vats_shift_ignores_friends);
    bool_tweak!("Tweaks", "bRadiationCausesRadioStatic", radiation_radio_static);
    bool_tweak!("Tweaks", "bSelectUnavailableRadios", select_unavailable_radios);
    bool_tweak!("Tweaks", "bNoSelfMeltdownDamage", no_self_meltdown);
    bool_tweak!("Tweaks", "bSneakAttackWithoutCrouching", standing_sneak_attacks);
    bool_tweak!("Tweaks", "bNoCompanionItemDamage", no_companion_item_damage);
    bool_tweak!("Tweaks", "bWeaponModdingSkillRequirement", modding_needs_skill);
    bool_tweak!("Tweaks", "bPlantingLiveGrenadesRequiresWeaponSkill", planting_needs_skill);
    bool_tweak!("Tweaks", "bFastTravelCostsSpecialNukaBottles", travel_costs_bottle);
    bool_tweak!("Tweaks", "bAllowUnsafeFastTravel", unsafe_fast_travel);
    bool_tweak!("Tweaks", "bArmorCausesSinking", armor_sinks);
    bool_tweak!("Tweaks", "bRememberBobbyPinHealth", remember_pin_health);
    bool_tweak!("Tweaks", "bForceLockpickNoBreakLock", force_spares_lock);
    bool_tweak!("Tweaks", "bNoWeaponConditionDamagePenalty", no_condition_damage_penalty);
    bool_tweak!("Tweaks", "bRemoveWeaponDamageBuffer", no_condition_damage_buffer);
    bool_tweak!("Tweaks", "bCritChanceIgnoresFireRate", crit_ignores_fire_rate);
    bool_tweak!("Tweaks", "bNoScaleNpcDamageResistanceByCondition", npc_dr_ignores_condition);
    bool_tweak!("Tweaks", "bNoScaleNpcDamageThresholdByCondition", npc_dt_ignores_condition);
    bool_tweak!("Tweaks", "bExplosionRadiusBuff", explosion_radius_buff);
    bool_tweak!("Tweaks", "bNPCsCanSneakCritPlayer", npcs_sneak_crit_player);
    bool_tweak!("Tweaks", "bFatalNonSneakCritsAlwaysGib", fatal_crits_gib);
    bool_tweak!("Tweaks", "bFallDeathsCauseDismemberment", fall_deaths_dismember);
    bool_tweak!("Tweaks", "bSplashDamageTorsoGibbing", splash_torso_gib);
    bool_tweak!("Tweaks", "bNPCsDropWeaponHolsteredWeapon", npcs_drop_holstered);
    bool_tweak!("Tweaks", "bNPCsEarnAmmoCasings", npcs_earn_casings);
    bool_tweak!("Tweaks", "bNoAmmoCasings", no_ammo_casings);
    bool_tweak!("Tweaks", "bCompareArmorStats", compare_armor);
    bool_tweak!("Tweaks", "bMoreDetailedStatsMenu", detailed_stats);
    bool_tweak!("Tweaks", "bTimescaleAffectsMinigames", minigames_timescale);
    bool_tweak!("Tweaks", "bNoPlayerNameLimit", no_name_limit);
    bool_tweak!("Tweaks", "bRecurringChallengeIndicator", mark_recurring_challenges);
    bool_tweak!("Tweaks", "bBetterMapZoom", map_zoom_at_pointer);
    bool_tweak!("Tweaks", "bModConsolePrintsIncludeName", print_plugin_names);
    bool_tweak!("Tweaks", "bWeaponRequirementsMatter", weapon_requirements_matter);
    bool_tweak!("Tweaks", "bDisarmRequiresSkill", disarm_needs_skill);
    bool_tweak!("Tweaks", "bCapSkillsBySPECIAL", cap_skills_by_special);
    bool_tweak!("Tweaks", "bEnemyHealthbarShowBuffedHP", buffed_health_bars);
    bool_tweak!("Tweaks", "bNPCsDetectMineExplosions", mine_blasts_heard);
    bool_tweak!("Tweaks", "bExplosionsDontPushTakeableItems", blasts_spare_items);
    bool_tweak!("Tweaks", "bColorRecentlyAddedMapMarkers", recent_markers_red);
    bool_tweak!("Tweaks", "bNoPlaceMarkerPopup", no_marker_popup);
    bool_tweak!("Tweaks", "bReloadingWithNoAmmoSwapsAmmoTypes", empty_reload_swaps_ammo);
    bool_tweak!("Tweaks", "bMenuSearch", menu_search);
    bool_tweak!("Tweaks", "bArmorConditionLabel", armor_condition);
    bool_tweak!("Tweaks", "bNoLockEncounterZoneLevels", no_lock_encounter_zone_levels);
    bool_tweak!("Tweaks", "bFasterSleepWait", faster_sleep_wait);
    bool_tweak!("Tweaks", "bUseWASDAsArrowKeys", wasd_as_arrow_keys);

    // Numeric [Tweaks] settings with documented domains.
    if let Some(n) = overrides.get_i32("Tweaks", "iPerksPerLevel").filter(|&n| n >= 1) {
        tweaks.perks_per_level = n.clamp(1, 5) as usize;
        applied += 1;
    }
    if let Some(n) = overrides.get_i32("Tweaks", "iMaxCharacterLevel").filter(|&n| n >= 0) {
        tweaks.max_level = n.clamp(0, 100) as u32;
        applied += 1;
    }
    if let Some(pct) = overrides.get_f64("Tweaks", "fCraftedItemHealthPct").filter(|v| v.is_finite() && *v >= 0.0) {
        tweaks.crafted_item_health_pct = (pct as f32).clamp(0.0, 100.0);
        applied += 1;
    }

    // Documented sub-sections (`docs/spec-notes/stewie-stat-profile.md`,
    // `docs/spec-notes/ext-small.md`, `engine/nvf-play/src/tweaks.rs`).
    if let Some(scale) = overrides.get_f64("Critical Hits", "fCriticalDamageMult").filter(|&v| v >= 0.0) {
        tweaks.crit_damage_scale = scale;
        applied += 1;
    }
    bool_tweak!("Hardcore Tweaks", "bAmmoWeight", hardcore_ammo_weight);
    bool_tweak!("Hardcore Tweaks", "bEssentialCompanions", hardcore_essential_companions);
    bool_tweak!("Hardcore Tweaks", "bSleepingHeals", hardcore_sleep_heals);

    applied
}

/// Parse a single GMST `key = value` entry into a typed `Setting` by its Editor ID
/// prefix (`f` -> `Float`, `i` -> `Int`, `s` -> `Str`) [documented: xEdit
/// `wbDefinitionsFNV.pas` GMST; `crates/fnv-sim/src/settings.rs`]. Unknown prefixes or
/// unparseable numeric values return `None`.
fn parse_gmst_value(key: &str, raw_val: &str) -> Option<Setting> {
    let val = strip_inline_comment(raw_val);
    match key.as_bytes().first().map(u8::to_ascii_lowercase) {
        Some(b'f') => {
            let f = parse_ini_f64(val)?;
            let f32_val = f as f32;
            f32_val.is_finite().then_some(Setting::Float(f32_val))
        }
        Some(b'i') => parse_ini_i32(val).map(Setting::Int),
        Some(b's') => Some(Setting::Str(unwrap_quotes(val))),
        _ => None,
    }
}

/// Parse GMST override lines from INI text into `out` (keyed by lower-case GMST name).
///
/// When `allow_headerless` is `true` (used for `Tweaks\GameSettings\*.ini`), top-level
/// lines before any section header and lines under `[GameSettings]` are accepted [design].
/// When `allow_headerless` is `false` (used for `[GameSettings]` inside
/// `Tweaks\INIs\*.ini`), only lines under an explicit `[GameSettings]` section are
/// accepted [design].
pub fn parse_game_settings_into(text: &str, allow_headerless: bool, out: &mut BTreeMap<String, Setting>) {
    let mut active = allow_headerless;
    for line in strip_bom_str(text).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            if let Some(sec) = parse_section_header(trimmed) {
                active = sec == "gamesettings";
            }
            continue;
        }
        if !active {
            continue;
        }
        let Some((raw_key, raw_val)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }
        if let Some(setting) = parse_gmst_value(key, raw_val) {
            out.insert(key.to_ascii_lowercase(), setting);
        }
    }
}

/// Load modular GMST overrides from explicit `roots` in deterministic case-insensitive
/// filename order (`A..Z`): `[GameSettings]` sections in `Tweaks\INIs\*.ini` first, then
/// `Tweaks\GameSettings\*.ini` on top [design]. Returns `(file_count, overrides)`.
pub fn load_modular_game_settings_from_roots(roots: &[PathBuf]) -> (usize, BTreeMap<String, Setting>) {
    let mut out = BTreeMap::new();
    let mut files = 0usize;
    for (_, text) in crate::mod_files::read_folder_inis_from_roots(roots, MODULAR_INIS_DIR) {
        let before = out.len();
        parse_game_settings_into(&text, false, &mut out);
        if out.len() != before {
            files += 1;
        }
    }
    for (_, text) in crate::mod_files::read_folder_inis_from_roots(roots, GAME_SETTINGS_DIR) {
        files += 1;
        parse_game_settings_into(&text, true, &mut out);
    }
    (files, out)
}

/// Load modular GMST overrides from the current `mod_files` read roots [design].
pub fn load_modular_game_settings_from_data() -> (usize, BTreeMap<String, Setting>) {
    let mut out = BTreeMap::new();
    let mut files = 0usize;
    for (_, text) in crate::mod_files::read_data_folder_inis(MODULAR_INIS_DIR) {
        let before = out.len();
        parse_game_settings_into(&text, false, &mut out);
        if out.len() != before {
            files += 1;
        }
    }
    for (_, text) in crate::mod_files::read_data_folder_inis(GAME_SETTINGS_DIR) {
        files += 1;
        parse_game_settings_into(&text, true, &mut out);
    }
    (files, out)
}

/// Load modular GMST overrides from both read roots and `mod_files` session writes
/// [design].
pub fn load_modular_game_settings(mod_files: &ModFiles) -> (usize, BTreeMap<String, Setting>) {
    let mut out = BTreeMap::new();
    let mut files = 0usize;
    for (_, text) in mod_files.read_folder_inis(MODULAR_INIS_DIR) {
        let before = out.len();
        parse_game_settings_into(&text, false, &mut out);
        if out.len() != before {
            files += 1;
        }
    }
    for (_, text) in mod_files.read_folder_inis(GAME_SETTINGS_DIR) {
        files += 1;
        parse_game_settings_into(&text, true, &mut out);
    }
    (files, out)
}

/// Apply parsed GMST overrides onto a `FormStore` (only updating existing load-order
/// GMSTs with matching prefix/kind; unknown keys are ignored) [design].
pub fn apply_game_settings_to_forms(forms: &mut FormStore, overrides: &BTreeMap<String, Setting>) -> usize {
    let mut applied = 0usize;
    for (name, value) in overrides {
        if forms.apply_setting_override(name, value.clone()) {
            applied += 1;
        }
    }
    applied
}

/// Apply parsed GMST overrides onto a `World` via `World::change_setting` (only updating
/// existing load-order GMSTs with matching prefix/kind; unknown keys are ignored)
/// [design].
pub fn apply_game_settings_to_world(world: &mut World<'_>, overrides: &BTreeMap<String, Setting>) -> usize {
    let mut applied = 0usize;
    for (name, value) in overrides {
        if world.change_setting(name, value.clone()).is_ok() {
            applied += 1;
        }
    }
    applied
}

/// Load modular GMST overrides from the current `mod_files` read roots and apply them to
/// `forms`. Returns `(file_count, applied_count)`.
pub fn apply_modular_game_settings_to_forms(forms: &mut FormStore) -> (usize, usize) {
    let (files, overrides) = load_modular_game_settings_from_data();
    if overrides.is_empty() {
        return (files, 0);
    }
    let applied = apply_game_settings_to_forms(forms, &overrides);
    (files, applied)
}

/// Load modular GMST overrides from explicit `roots` and apply them to `forms`. Returns
/// `(file_count, applied_count)`.
pub fn apply_modular_game_settings_to_forms_from_roots(forms: &mut FormStore, roots: &[PathBuf]) -> (usize, usize) {
    let (files, overrides) = load_modular_game_settings_from_roots(roots);
    if overrides.is_empty() {
        return (files, 0);
    }
    let applied = apply_game_settings_to_forms(forms, &overrides);
    (files, applied)
}

/// Load modular GMST overrides from the current `mod_files` read roots and apply them to
/// `world`. Returns `(file_count, applied_count)`.
pub fn apply_modular_game_settings_to_world(world: &mut World<'_>) -> (usize, usize) {
    let (files, overrides) = load_modular_game_settings_from_data();
    if overrides.is_empty() {
        return (files, 0);
    }
    let applied = apply_game_settings_to_world(world, &overrides);
    (files, applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stewie_ini_with_bom_comments_quotes_and_later_overrides() {
        let mut map = StewieIniMap::default();
        map.parse_into(
            "\u{feff}; leading comment\r\n\
             [Tweaks]\r\n\
             bMenuSearch = 1 ; enable search\r\n\
             bNoCasinoBans = 0\r\n\
             malformed line without equals\r\n\
             = missing_key\r\n\
             [Cookable Grenades]\r\n\
             sSoundName = \"Beep;Tone\" ; inline comment outside quotes\r\n",
        );
        map.parse_into(
            "# second file overrides one key and preserves the rest\n\
             [tweaks]\n\
             bnocasinobans=1\n\
             [Critical Hits]\n\
             fCriticalDamageMult = 2.25\n",
        );

        assert_eq!(map.get_bool("Tweaks", "bMenuSearch"), Some(true));
        assert_eq!(map.get_bool("Tweaks", "bNoCasinoBans"), Some(true));
        assert_eq!(map.get("Cookable Grenades", "sSoundName"), Some("\"Beep;Tone\""));
        assert_eq!(map.get_f64("Critical Hits", "fCriticalDamageMult"), Some(2.25));
        assert_eq!(map.get("Tweaks", "bUnknown"), None);
    }

    #[test]
    fn parses_game_settings_headerless_and_sectioned_and_ignores_malformed() {
        let mut out = BTreeMap::new();
        parse_game_settings_into(
            "\u{ef}\u{bb}\u{bf}; headerless GMST file\n\
             fMoveBaseSpeed = 95.5 ; faster walk\n\
             iHoursToRespawnCell = 48.0\n\
             sOk = \"Accept; Go\" ; keep semicolon inside quotes\n\
             [OtherSection]\n\
             fMoveBaseSpeed = 999.0\n\
             [GameSettings]\n\
             iHoursToRespawnCell = 24\n\
             sGreeting = Hello ; trimmed comment\n\
             fBadFloat = not_a_number\n\
             fInfFloat = inf\n\
             iBadInt = nope\n\
             bUnsupportedPrefix = 1\n\
             broken line\n",
            true,
            &mut out,
        );

        assert_eq!(out.get("fmovebasespeed"), Some(&Setting::Float(95.5)));
        assert_eq!(out.get("ihourstorespawncell"), Some(&Setting::Int(24)));
        assert_eq!(out.get("sok"), Some(&Setting::Str("Accept; Go".into())));
        assert_eq!(out.get("sgreeting"), Some(&Setting::Str("Hello".into())));
        assert!(!out.contains_key("fbadfloat"));
        assert!(!out.contains_key("finffloat"));
        assert!(!out.contains_key("ibadint"));
        assert!(!out.contains_key("bunsupportedprefix"));
    }

    #[test]
    fn apply_modular_tweaks_updates_recognized_fields_and_ignores_unknown_and_invalid() {
        let mut map = StewieIniMap::default();
        map.parse_into(
            "[Tweaks]\n\
             bCritChanceIgnoresFireRate=1\n\
             bMenuSearch=1\n\
             bNoLockEncounterZoneLevels=1\n\
             bHardcoreTweaks=1\n\
             iPerksPerLevel=3\n\
             iMaxCharacterLevel=50\n\
             bNoCasinoBans=not_a_bool\n\
             bCompletelyUnknownTweak=1\n\
             [Critical Hits]\n\
             fCriticalDamageMult=1.75\n\
             [Hardcore Tweaks]\n\
             bAmmoWeight=0\n\
             bEssentialCompanions=1\n\
             bSleepingHeals=1\n",
        );

        let mut tweaks = Tweaks::default();
        let count = apply_modular_tweaks_to_game(&map, &mut tweaks);
        assert_eq!(count, 10);
        assert!(tweaks.crit_ignores_fire_rate);
        assert!(tweaks.menu_search);
        assert!(tweaks.no_lock_encounter_zone_levels);
        assert!(tweaks.hardcore_tweaks);
        assert!(!tweaks.hardcore_ammo_weight);
        assert!(tweaks.hardcore_essential_companions);
        assert!(tweaks.hardcore_sleep_heals);
        assert_eq!(tweaks.perks_per_level, 3);
        assert_eq!(tweaks.max_level, 50);
        assert_eq!(tweaks.crit_damage_scale, 1.75);
        assert!(!tweaks.no_casino_bans, "invalid bool must leave default unchanged");
    }

    #[test]
    fn apply_game_settings_to_forms_enforces_existence_and_kind() {
        let mut forms = FormStore::default();
        forms.settings.insert("ftestfloat".into(), Setting::Float(1.0));
        forms.settings.insert("itestint".into(), Setting::Int(10));
        forms.settings.insert("steststr".into(), Setting::Str("Before".into()));

        let mut overrides = BTreeMap::new();
        overrides.insert("ftestfloat".into(), Setting::Float(3.25));
        overrides.insert("itestint".into(), Setting::Int(42));
        overrides.insert("steststr".into(), Setting::Str("After".into()));
        overrides.insert("funknown".into(), Setting::Float(9.0));
        overrides.insert("sunknown".into(), Setting::Str("Ignored".into()));

        let applied = apply_game_settings_to_forms(&mut forms, &overrides);
        assert_eq!(applied, 3);
        assert_eq!(forms.settings.get("ftestfloat"), Some(&Setting::Float(3.25)));
        assert_eq!(forms.settings.get("itestint"), Some(&Setting::Int(42)));
        assert_eq!(forms.settings.get("steststr"), Some(&Setting::Str("After".into())));
        assert!(!forms.settings.contains_key("funknown"));
        assert!(!forms.settings.contains_key("sunknown"));
    }
}
