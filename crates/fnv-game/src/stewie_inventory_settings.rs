//! The existing inventory settings queried by mods through StewVal.
//! See docs/spec-notes/ext-small.md, "Live inventory settings". The caller supplies
//! the engine's authoritative settings only after their startup handoff.

use crate::Game;

impl Game<'_> {
    pub(crate) fn loaded_stewie_inventory_value(&self, key: &str, section: &str) -> Option<f64> {
        if !self.small.stewie_stat_settings_loaded {
            return None;
        }
        if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("fCraftedItemHealthPct") {
            return Some(f64::from(self.tweaks.crafted_item_health_pct));
        }
        let chosen = if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bHardcoreTweaks") {
            self.tweaks.hardcore_tweaks
        } else if section.eq_ignore_ascii_case("Hardcore Tweaks") && key.eq_ignore_ascii_case("bAmmoWeight") {
            // This is the retained child choice. The inventory consumer applies the
            // parent switch separately in Game::sync_perk_weights.
            self.tweaks.hardcore_ammo_weight
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bIndividialItemStats") {
            self.tweaks.single_item_figures
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bDisableStealthEffectInPipboy") {
            self.tweaks.no_stealth_shimmer_in_pipboy
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bKeepPipboyLightOnCellChange") {
            // NVForever already keeps the Pip-Boy light on across cell changes (docs/design/tweaks.md).
            true
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bWeightlessWornArmor") {
            self.tweaks.weightless_worn_armor
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bWeightlessWornPowerArmor") {
            self.tweaks.weightless_worn_power_armor
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bSynchronizeContainerCategories") {
            self.tweaks.sync_categories
        } else if section.eq_ignore_ascii_case("Synchronized Container Categories")
            && key.eq_ignore_ascii_case("bPreventSwitchWhenEmptyingCategory")
        {
            // Retained child choice under the synchronized-categories parent switch.
            self.tweaks.prevent_switch_when_emptying_category
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bBarterCheckActorBuySellFlags") {
            self.tweaks.vendor_lists_both
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bDontOpenDoorsAfterLockpick") {
            self.tweaks.no_door_after_pick
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bNoSkipDialog") {
            self.tweaks.no_skip_dialogue
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bRestoreLockedDoorSound") {
            // NVForever already plays DRSLocked when activating a refused lock (docs/design/tweaks.md).
            true
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bArmorConditionLabel") {
            self.tweaks.armor_condition
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bNoLockEncounterZoneLevels") {
            self.tweaks.no_lock_encounter_zone_levels
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bFasterSleepWait") {
            self.tweaks.faster_sleep_wait
        } else if section.eq_ignore_ascii_case("Tweaks") && key.eq_ignore_ascii_case("bUseWASDAsArrowKeys") {
            self.tweaks.wasd_as_arrow_keys
        } else {
            return None;
        };
        Some(f64::from(u8::from(chosen)))
    }
}
