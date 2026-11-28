//! The settings page's Tweaks tab (docs/design/tweaks.md): optional changes to how the
//! game plays or what it shows, each off (or at "no change") unless the player sets it.
//! The list follows the features of Stewie's Tweaks, a script-extender plugin for the
//! original, each written here from what the plugin's readme says it does; neither its
//! code nor its files are used, and the labels and notes are NVForever's own words. A
//! row's note names the plugin's INI key it stands for, so a player who knows the plugin
//! can find theirs. Rows are kept in `NVForever.ini` `[Settings]` as every row is
//! (`crate::settings`), under keys starting `tweak-`.
//!
//! The values live in [`TweakSettings`]: the game's rules' part (`fnv_game::tweaks`) is
//! handed to the running game each frame ([`sync`], as the difficulty is: the game is
//! made after the settings are read, and again for a loaded save); the engine's own
//! modules read the rest from the resource.

use crate::script::ScriptState;
use crate::settings::{add_choice_list, add_setting, choice_index, choice_of, choice_part, ChoiceList, Setting, Slider};
use bevy::prelude::*;
use fnv_game::tweaks::Tweaks;

/// The settings page's group (its tab) for the tweaks.
pub const GROUP: &str = "Tweaks";

/// Turn Further While Seated, for the player's body (the nvforever crate), whose systems
/// have no room for another parameter [design: docs/design/tweaks.md].
pub static SEATED_WIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// No Turning Animation, for the player's body [design: docs/design/tweaks.md].
pub static NO_TURN_CLIP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Remove Landing Animation, for the player's body [design: docs/design/tweaks.md].
pub static NO_LAND_CLIP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Activating Doesn't Stand Up: on furniture only a movement control gets the player up
/// [design: docs/design/tweaks.md].
pub static SEAT_KEEPS_ON_ACTIVATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Armor Sounds Play in 3D, for the player's foley (nvforever `anim_sounds`) [design: docs/design/tweaks.md].
pub static ARMOR_3D: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The Toggle Sighting Node key has the iron sights on the weapon's second sighting node
/// (nvforever `player_body`, which clears it for a new weapon) [design: docs/design/tweaks.md].
pub static SECOND_SIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Force Hi-Res Weapon Models, for the actors' weapons (nvforever `actors`) [design: docs/design/tweaks.md].
pub static HI_RES_NPC_WEAPONS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The sound lists' first choice: the game's own sound.
const GAME_SOUND: &str = "(the game's own)";

/// The plugins' interface sounds by Editor ID (those starting `UI`), sorted, after
/// [`GAME_SOUND`]: what a sound list of the Tweaks tab offers.
fn sound_names(w: &World) -> Vec<String> {
    let mut v: Vec<String> = w.get_resource::<crate::sfx::Sfx>().map(|s| s.forms.sounds.values().filter_map(|x| x.edid.clone()).filter(|e| e.starts_with("UI")).collect()).unwrap_or_default();
    v.sort();
    v.dedup();
    v.insert(0, GAME_SOUND.to_string());
    v
}

/// A chosen name as kept: empty for [`GAME_SOUND`].
fn sound_kept(name: &str) -> String {
    if name == GAME_SOUND {
        String::new()
    } else {
        name.trim().to_string()
    }
}

/// Every tweak's value. `Default`: the original's behaviour.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct TweakSettings {
    /// The part the game's rules read (`fnv_game::Game::tweaks`).
    pub game: Tweaks,
    /// `bNoKarmaMessages`: karma changes put up no corner message and play no sound.
    pub no_karma_messages: bool,
    /// `bNoXPMessages`: experience gains do not bring up the XP meter or its sound.
    pub no_xp_messages: bool,
    /// `bNoQuestFailedPopup`: no "Quest failed" banner.
    pub no_quest_failed_popup: bool,
    /// `bNoLocationPopup`: no banner when a place is discovered.
    pub no_location_popup: bool,
    /// `bRemoveQuestObjectiveAddedText`: no list of new objectives on the HUD.
    pub no_objective_popup: bool,
    /// `bDisableFastTravel`: fast travel is always refused.
    pub disable_fast_travel: bool,
    /// `bMidairFastTravel`: fast travel is not refused for being in the air.
    pub midair_fast_travel: bool,
    /// `bFastTravelWithEnemiesNearby`: fast travel is not refused for enemies nearby.
    pub fast_travel_enemies_nearby: bool,
    /// `bFastTravelOverencumbered`: fast travel is not refused for carrying too much.
    pub fast_travel_overencumbered: bool,
    /// `bNoRedCrosshairOnEnemies`: the crosshair does not turn red over a hostile actor.
    pub no_red_crosshair: bool,
    /// `bHideEnemyMarkers`: hostile actors are not shown on the compass.
    pub hide_enemy_markers: bool,
    /// `bDisableCombatMusic`: no battle music.
    pub no_combat_music: bool,
    /// `bDisableLoadingScreenTips`: loading screens show no tip.
    pub no_loading_tips: bool,
    /// `bNoReputationMessages`: reputation changes put up no box and play no sound.
    pub no_reputation_messages: bool,
    /// `bDisableExplosionInFaceIMOD`: a nearby explosion does not blur the screen.
    pub no_explosion_blur: bool,
    /// `bRemoveSneakLabel`: no [UNSEEN] / [SPOTTED] label while sneaking.
    pub no_sneak_label: bool,
    /// `bVatsExitKey`: a right click ends a kill camera.
    pub kill_cam_exit_key: bool,
    /// `bNumberedComputerHotkeys`: 1 to 9 and 0 choose a terminal's rows.
    pub numbered_terminal_keys: bool,
    /// `bDeselectHotkeys`: assigning an item to the hotkey it has takes it off.
    pub deselect_hotkeys: bool,
    /// `bScaleMusicVolumeDuringDialogue`/`fDialogueMusicVolumeMult`: the music's volume
    /// in a conversation, as a part of its own; 1 changes nothing.
    pub dialogue_music_volume: f32,
    /// `bScaleRadioSongVolumeDuringDialogue`/`fDialogueSongVolumeMult`: the radio's.
    pub dialogue_radio_volume: f32,
    /// `bNonSelectablePlayerInConsole`: a console click never picks the player.
    pub console_skips_player: bool,
    /// `bPipboyLightAndHolsteringIgnoreTimescale`: holding Ready Item to holster counts
    /// real time (the Pip-Boy light already does).
    pub holster_real_time: bool,
    /// `bPowerArmorPreventsScreenBlood`: no blood on the screen in power armour.
    pub power_armor_no_screen_blood: bool,
    /// `bHideRedCrosshairOnDistantInvisibleTargets`: an invisible foe does not turn the
    /// crosshair red.
    pub no_red_crosshair_on_cloaked: bool,
    /// `bGodmodePreventsLegCrippleSound`: no hard-landing sound for the player in god mode.
    pub god_mode_quiet_landing: bool,
    /// `bCompressDuplicateConsoleMessages`: a console line said again right after
    /// itself is counted on the same line.
    pub console_compress_repeats: bool,
    /// `bConsolePrintsIncludeTimestamp`: console lines start with the time since the
    /// game started.
    pub console_timestamps: bool,
    /// `bLivingAnatomyShowDR`: Living Anatomy's line shows the DR too.
    pub living_anatomy_dr: bool,
    /// `bNoFastTravelTimeChange`: fast travel takes no time.
    pub no_travel_time: bool,
    /// `bPowerArmorScalesFallDamage`/`fFallDamageMult`: fall damage to anyone in power
    /// armour is multiplied by this; 1 changes nothing.
    pub power_armor_fall_damage: f32,
    /// `bNoFastTravelIfLegsCrippled`: fast travel is refused with a crippled leg.
    pub no_travel_crippled_leg: bool,
    /// `bJumpingCostsAP`/`iJumpAPCost`: Action Points a jump costs; 0 changes nothing.
    pub jump_ap_cost: f32,
    /// `[Jumping Costs AP] bCombatOnly`: jumps only cost AP while in danger (Stewie 9.70).
    pub jump_ap_danger_only: bool,
    /// `[Jumping Costs AP] bJumpWithoutEnoughAP`: allow jumping with less AP than the cost,
    /// draining remaining AP to 0 instead of refusing.
    pub jump_without_enough_ap: bool,
    /// `bRunningCostsAP`: Action Points running costs a second; 0 changes nothing.
    pub run_ap_per_second: f32,
    /// `bRunningCostsAP` (`fSneakAPDrainCostBase`, `fSneakRunAPEnduranceMult`): Action
    /// Points running while sneaking costs a second (`fSneakAPDrainCostBase +
    /// fSneakRunAPEnduranceMult * (Endurance - 5)`) [documented: Stewie Tweaks v10.05
    /// generated `nvse_stewie_tweaks.ini` `[Running Costs AP]` comment; option names also
    /// in v10.05 Readme & `AllTweaks.json`; hypothesis: sign vs. the v10.05 Readme's
    /// `(5 - Endurance)` standing-run formula is unobserved in the original binary;
    /// design: clamp at 0, off when the base is 0, Endurance fallback of 5].
    pub sneak_run_ap_base: f32,
    pub sneak_run_ap_endurance_mult: f32,
    /// `bDisableSellingItems`: the player cannot sell in a barter.
    pub no_selling: bool,
    /// The Own Skill Points Rule's numbers, kept while the rule is off.
    pub skill_point_base: i32,
    pub skill_points_per_int: f64,
    /// `bAimingSpeed`: aiming or blocking does not slow the player to a walk.
    pub no_aiming_slowdown: bool,
    /// `bRepeatJumping`: holding Jump jumps again on landing.
    pub repeat_jumping: bool,
    /// `bHoldCrouchToSneak`: sneak while the key is held.
    pub hold_to_sneak: bool,
    /// `bChargedAttacksCostAP`/`iChargedAttackAPCost`: Action Points a power attack
    /// costs; 0 changes nothing.
    pub power_attack_ap: f32,
    /// `bDistanceBasedQuestMarkerVisibilty`: quest targets farther than this are left off
    /// the compass; 0 changes nothing.
    pub quest_marker_reach: f32,
    /// `bUseCustomSniperZoomRate`/`fScopeFOVTimeChange`: a scope's zoom time as a part of
    /// the usual; 1 changes nothing.
    pub scope_zoom_time: f32,
    /// `bAddInventoryDropItemHotkey`: Q drops the selected Pip-Boy item.
    pub drop_key: bool,
    /// `bAimingUnholstersWeapons`: aiming draws a holstered weapon.
    pub aiming_draws_weapon: bool,
    /// `bBillyGoatMode`/`fWalkAngle`: the steepest slope the player walks up, degrees.
    pub walk_angle: f32,
    /// `bCrippledLegsScaleJumpHeight`: the jump height is multiplied by this for each
    /// crippled leg; 1 changes nothing.
    pub crippled_leg_jump: f32,
    /// `bNoPlayerStaggerAnims`: the player never staggers from a hit.
    pub no_player_stagger: bool,
    /// `bPreventReloadWeaponUnholstering`: Ready Item does not draw a holstered weapon.
    pub ready_does_not_draw: bool,
    /// `bReloadingPreventsWeaponHotkeys`: weapon hotkeys do nothing while reloading.
    pub no_hotkeys_while_reloading: bool,
    /// `bRunSlowerInWater`/`fWadingMovementMult`: the pace at a swimming depth of water,
    /// as a part of the usual (shallower, less slowed); 1 changes nothing.
    pub wading_pace: f32,
    /// `bMinBloodSplatterHealthDamage`/`fMinHealthDamage`: hits that take less health
    /// put no blood on the screen; 0 changes nothing.
    pub min_splatter_damage: f32,
    /// `bMinigamesPlayXPSoundAtMaxLevel`: the experience sound plays at the level cap.
    pub xp_sound_at_cap: bool,
    /// `bHoldWaitKeyToShowMenu`: the Wait key is held to open the menu.
    pub hold_to_wait: bool,
    /// `bDisableWeaponFOV`: aiming down iron sights does not zoom.
    pub no_sight_zoom: bool,
    /// `bHideMiscQuestItems`: quest items are left out of the Pip-Boy's Misc list.
    pub hide_misc_quest: bool,
    /// `bClickingActiveTabTogglesSorting`: a click on the Pip-Boy ITEMS tab already open
    /// steps the list's order (name, value, weight).
    pub tab_click_sorts: bool,
    /// `bHotkeyHolstersWeaponIfEquipped`: the hotkey of the weapon in hand draws or
    /// holsters it.
    pub hotkey_toggles_in_hand: bool,
    /// `bDisableInteriorFog`: interior fog is pushed out of sight (from the next cell).
    pub no_interior_fog: bool,
    /// `bNoCompassPipsIfNotInDanger`: actors are on the compass only in danger.
    pub compass_pips_in_danger_only: bool,
    /// `bHostilesUseNeutralColorOnCompass`: foes on the compass in the usual colour.
    pub neutral_foes_on_compass: bool,
    /// `bShowSneakLabelWhileStanding`: the sneak label shows while standing too.
    pub sneak_label_standing: bool,
    /// `bNoXPBarInCombat`: the XP meter waits for the fight to end.
    pub no_xp_meter_in_combat: bool,
    /// `bDisableHUDCrippledLimbIndicator`: no crippled-limb figure on the HUD.
    pub no_limb_indicator: bool,
    /// `bSkipDeathcamHotkey` (`fAttackDebounceTime`): Left Alt ends the death camera at
    /// once; so does an attack this many seconds after the death (0: not by attacking).
    pub skip_death_cam: bool,
    pub skip_death_cam_attack: f32,
    /// `bReplaceHelpWithConsole` (`[Help Button Replacer] sButtonLabel` [documented:
    /// Stewie Tweaks v10.05 Readme & generated `nvse_stewie_tweaks.ini`]): the pause menu
    /// has a Console entry where the original has Help, using `help_console_label` when
    /// non-empty (else `"Console"`, read at startup from `NVForever.ini` `[Settings]
    /// tweak-help-console-label` [design]).
    pub help_is_console: bool,
    pub help_console_label: String,
    /// `bHeatbeatSoundsFade` (`fDuration`): the low-health heartbeat fades out over this
    /// many seconds, whole again at each loss of health.
    pub heartbeat_fade: bool,
    pub heartbeat_fade_seconds: f32,
    /// `bTerminalFadeTime` (`fFadeLength`): a closing terminal fades out over this many
    /// seconds instead of the menu file's 0.25.
    pub terminal_fade: bool,
    pub terminal_fade_seconds: f32,
    /// `bContainersShowEquippedAmmo`: the loaded ammo has the equipped mark in a
    /// container's player column too.
    pub containers_show_ammo: bool,
    /// `bDescriptiveMarkerAddedMessage`: the map-marker-added message names the place.
    pub descriptive_marker_added: bool,
    /// `bDisableGrenadeIndicator`: no grenade indicator on the HUD.
    pub no_grenade_indicator: bool,
    /// `bHideGrenadeIndicatorForNoDamageExplosions`: grenades whose explosion does no
    /// damage are not marked.
    pub grenade_indicator_skip_harmless: bool,
    /// `bExplodingDestructibleIndicator`: objects burning toward an explosion are marked too.
    pub grenade_indicator_destructibles: bool,
    /// `bContainerTakeAllDoesntCloseMenu`: Take All leaves the container open.
    pub take_all_stays: bool,
    /// `bTakeAllConfirmation`: Take All of at least `take_all_confirm_min` items asks to be
    /// pressed again.
    pub take_all_confirm: bool,
    pub take_all_confirm_min: f32,
    /// `bRepairAllConfirmation`: Repair All costing `repair_all_threshold` caps or more asks
    /// to be pressed again.
    pub repair_all_confirm: bool,
    pub repair_all_threshold: f32,
    /// `bHideEquippedItemsInBarter`: equipped items are not offered in barter.
    pub hide_equipped_barter: bool,
    /// `bHideEquippedItemsInContainers`: equipped items are not listed in containers.
    pub hide_equipped_containers: bool,
    /// `bNoCapitaliseContainerCategories`: category titles in title case.
    pub title_case_categories: bool,
    /// `bDisableInventoryRightClickDrop`: a right click in the inventory does not drop.
    pub no_right_click_drop: bool,
    /// `bPreventNoFastTravelMessage`: an undiscovered marker refuses travel quietly.
    pub quiet_undiscovered: bool,
    /// The crosshair is hidden by its key (`Control::ToggleCrosshair`; not kept).
    pub crosshair_hidden: bool,
    /// `bTurnSlowerWhileAiming`/`fAimingScaleX`/`fAimingScaleY`: look speed while
    /// aiming, as a part of the usual, across and up and down; 1 changes nothing.
    pub aim_turn_x: f32,
    pub aim_turn_y: f32,
    /// `bScaleRadioVolume`/`fMusicScale`/`fDialogueScale`: the radio's songs and its
    /// talk, each as a part of their volume; 1 changes nothing.
    pub radio_music_volume: f32,
    pub radio_talk_volume: f32,
    /// `bOnlyChangeCameraHeightIfPOVKeyHeld`: the wheel moves the third-person camera
    /// only with Toggle View held.
    pub camera_wheel_needs_pov: bool,
    /// `bNoHotkeyEquipSounds`: weapon hotkeys make no equip sound.
    pub quiet_hotkeys: bool,
    /// `bDisableWeaponHotkeys`: the number keys and the D-pad pick no weapon.
    pub no_weapon_hotkeys: bool,
    /// `bOpenPipboyToInventoryByDefault`: the Pip-Boy opens on the inventory.
    pub pipboy_opens_items: bool,
    /// `bHideCrosshairWhileReloading`: no crosshair while reloading.
    pub hide_crosshair_reloading: bool,
    /// `bInvertCameraX`: looking left and right the other way.
    pub invert_x: bool,
    /// `bShowNearestUndiscoveredLocation`: the nearest place not yet found shows on the
    /// compass at any distance.
    pub nearest_unfound_on_compass: bool,
    /// `bLocationDiscoveredCornerMessage`: a place found is a corner line, not a banner.
    pub discovered_in_corner: bool,
    /// `bInvertContainerTitleScrollwheelDirection`: the wheel over a category title
    /// steps the other way.
    pub invert_title_wheel: bool,
    /// `bAutoContinueGame`: the main menu continues the newest save at once.
    pub auto_continue: bool,
    /// `bMainMenuContinueIcon`: the main menu shows the newest save's picture while
    /// Continue is chosen.
    pub continue_shows_save: bool,
    /// `bQuestTextVisibleInMenus`: the quest and location lines stay up over the Pip-Boy
    /// and a terminal.
    pub quest_text_in_menus: bool,
    /// `bDetectedByWhom`: `[DETECTED]` names who detects the player, up to
    /// `detected_names` names (0: only how many).
    pub detected_by_whom: bool,
    pub detected_names: f32,
    /// `bKeyRepeatAcceleration`: a held menu key repeats faster, `key_repeat_mult` times
    /// after `key_repeat_delay` seconds and `key_repeat_mult_alt` after
    /// `key_repeat_delay_alt` (`crate::key_repeat`).
    pub key_repeat_accel: bool,
    /// `bHUDFatigueIndicator`: an FTG word after the HUD's need words while the player's
    /// Fatigue is below full (`crate::hud_needs`).
    pub hud_fatigue: bool,
    /// `bToggleControllerIfAttackPressed`: a controller's A switches the controls to it, a
    /// left click back to the keyboard and mouse.
    pub toggle_device: bool,
    /// `bScaleExplosionShake`: the explosions' camera shake times `explosion_shake`
    /// (`crate::shake`; 0 for none).
    pub scale_explosion_shake: bool,
    pub explosion_shake: f32,
    pub key_repeat_delay: f32,
    pub key_repeat_mult: f32,
    pub key_repeat_delay_alt: f32,
    pub key_repeat_mult_alt: f32,
    /// `bMorePreciseRadMeter`: the HUD's RAD/SEC number with `rad_decimals` places.
    pub precise_rad_meter: bool,
    pub rad_decimals: u8,
    /// `bShowDoorsOnCompass`: doors leading elsewhere on the compass, within
    /// `compass_doors_exterior` / `compass_doors_interior` units, fading with distance
    /// under `compass_doors_fade`.
    pub compass_doors: bool,
    pub compass_doors_exterior: f32,
    pub compass_doors_interior: f32,
    pub compass_doors_fade: bool,
    /// `bArmorConditionLabel`: thresholds and placement for the body armour's condition
    /// row under the weapon's on the HUD (the enable switch itself lives on
    /// `game.armor_condition` so `StewVal`/`StewieGetValue` reads it live): shown below
    /// `armor_visible_below` (0: always), red below `armor_red_below`, flashing below
    /// `armor_flash_below`, in the weapon's place with none drawn under
    /// `armor_in_weapon_place`.
    pub armor_visible_below: f32,
    pub armor_red_below: f32,
    pub armor_flash_below: f32,
    pub armor_in_weapon_place: bool,
    /// `bHideAmmoLabel`: no ammo counter on the HUD.
    pub hide_ammo_label: bool,
    /// `bNoFreeBarterItems`/`fFreeItemCost`: the price of things worth nothing in a
    /// barter; 0 changes nothing.
    pub free_item_price: f32,
    /// `bUseCustomBarterPriceMultipliers`: a price multiplier for each kind of item, selling
    /// and buying; purified water keeps its price under `barter_water_unscaled`.
    pub barter_by_kind: bool,
    pub barter_water_unscaled: bool,
    pub barter_sell_armor: f32,
    pub barter_sell_weapon: f32,
    pub barter_sell_aid: f32,
    pub barter_sell_ammo: f32,
    pub barter_sell_misc: f32,
    pub barter_sell_weapon_mod: f32,
    pub barter_buy_armor: f32,
    pub barter_buy_weapon: f32,
    pub barter_buy_aid: f32,
    pub barter_buy_ammo: f32,
    pub barter_buy_misc: f32,
    pub barter_buy_weapon_mod: f32,
    /// `bAllowDuplicateControlBinds`: one input may serve two controls.
    pub duplicate_binds: bool,
    /// `bTogglePOVWhenHolsteringWeapon`: drawing goes to first person, holstering to third.
    pub pov_follows_weapon: bool,
    /// `bChanceBasedKillcams`/`fChance`: the share of kill cameras that play; 1 changes
    /// nothing.
    pub kill_cam_chance: f32,
    /// `bContainerEncumbranceIndicator`: the weight in red when taking would overload.
    pub warn_encumbrance: bool,
    /// `bHapticPipboy`: a short controller buzz on a Pip-Boy page or tab change.
    pub pipboy_buzz: bool,
    /// `bPowerAttackIfBlocking`: while blocking, a press of Use is a power attack at once.
    pub power_attack_if_blocking: bool,
    /// `bVATSHipFire`: V.A.T.S. entered without aiming fires every shot from the hip.
    pub vats_hip_fire: bool,
    /// `bNoPipBoyInCombat`: the Pip-Boy does not open while something fights the player.
    pub no_pipboy_in_combat: bool,
    /// `bNoEquippedWeaponMovementPenalty`: a drawn two-handed weapon does not slow the run.
    pub no_weapon_move_penalty: bool,
    /// `bUnequipBrokenArmor`: the player's armour comes off when it breaks.
    pub unequip_broken_armor: bool,
    /// `bHideCrosshairInFirstPerson`, `bHideCrosshairInThirdPerson`: no crosshair in that
    /// view; the activation prompt stays.
    pub hide_crosshair_first: bool,
    pub hide_crosshair_third: bool,
    /// `bColoredHUDBars` (`fRedHealthThreshold`, `fRedActionPointsThreshold`): the HP and
    /// AP bars turn red below these shares; 0 never.
    pub red_hp_below: f32,
    pub red_ap_below: f32,
    /// `bColorWeaponCndLabel` (`fColorThreshold`, `fFlashThreshold`): the weapon's
    /// condition bar red below the first share, flashing red below the second; 0 never.
    pub weapon_cnd_red_below: f32,
    pub weapon_cnd_flash_below: f32,
    /// `bBlackJackTotalDisplay`: the Blackjack screen shows the player's hand's total.
    pub blackjack_total: bool,
    /// `bSlotsAutoSpinHotkey`: W held spins the slot machine again each time it stops.
    pub slots_hold_to_spin: bool,
    /// `bTalkWhileSneakingIfShiftIsHeld`: Shift held while sneaking talks to a person
    /// instead of picking their pocket.
    pub shift_talks_sneaking: bool,
    /// `bTabClosesTerminals`: Tab closes a terminal from any of its pages.
    pub tab_closes_terminals: bool,
    /// `bTabBackInStartMenu`: Tab goes back in the pause menu where it steps no tabs.
    pub tab_back_in_pause: bool,
    /// `iDateFormat`: the style of `crate::wait::model::DATE_STYLES`.
    pub date_style: usize,
    /// `bEnteringVATSDoesntUnholsterWeapon`: a weapon V.A.T.S. drew goes away again when
    /// the mode ends with nothing fired.
    pub vats_keeps_holstered: bool,
    /// `bNoHUDHotkeyPopup`: holding a hotkey does not open the weapon wheel.
    pub no_hotkey_wheel: bool,
    /// `bMoveAmmoTypeLabel`: the HUD's ammo label under the ammo count.
    pub ammo_label_under: bool,
    /// `bCompanionPipColorChange`: which of [`COMPANION_COLOURS`] companions' compass
    /// pips take (0: as anyone's).
    pub companion_pip: usize,
    /// `bRaceMenuAllowFemaleFacialHair`: the character menus offer facial hair to women.
    pub female_facial_hair: bool,
    /// `bContainerMenuStoreAllHotkey`: Shift with Take All puts everything listed on the
    /// player's side in the container.
    pub store_all: bool,
    /// `bStartMenuQuickLoad`: the QuickLoad key works in the main and pause menus.
    pub quick_load_in_menus: bool,
    /// `bPickLocksEvenWithKey`: with Shift held a lock the player has the key of is
    /// picked instead.
    pub pick_locks_with_key: bool,
    /// `iToggleSmoothCameraKey`: the look eases after the input (the Smooth Camera key
    /// flips it; not kept in the INI).
    pub smooth_look: bool,
    /// `bRightClickChangesToPreviousContainerCategory`: a right click on a container's
    /// or a barter's list heading steps back a category.
    pub right_click_previous_category: bool,
    /// `bShowBarterTotalWhenOverSellLimit`: the barter figure shows the whole total when
    /// it is more than the merchant's caps.
    pub full_barter_total: bool,
    /// `bCompassFadeLeftSide`: compass marks fade toward the left end.
    pub compass_fade_left: bool,
    /// `bDisableControllerDeadzones` (`iControllerDeadzone`, `iControllerDeadzoneRS`): the
    /// sticks' own dead zones, as shares of a full push (0 none).
    pub custom_dead_zones: bool,
    pub left_dead_zone: f32,
    pub right_dead_zone: f32,
    /// `bControllerTriggerDeadzones` (`iDeadzoneLT`, `iDeadzoneRT`): how far each trigger
    /// goes down before it counts as pressed.
    pub custom_trigger_dead_zones: bool,
    pub lt_dead_zone: f32,
    pub rt_dead_zone: f32,
    /// `bMoreResponsiveControllerAiming`: the right stick turns in a straight line with
    /// the push past the dead zone.
    pub responsive_stick: bool,
    /// `bFasterControllerPOVRotate`: a controller circles the camera (Toggle POV held)
    /// twice as fast.
    pub faster_pad_orbit: bool,
    /// `bVanityCamReturnsToFirstPerson`: after the vanity camera (idle, or Toggle POV
    /// held) a first-person view goes back to first person.
    pub vanity_returns_first: bool,
    /// `bSeparateHorizontalSensitivity`: a factor on the look's left and right speed only.
    pub horizontal_look: f64,
    /// `bRememberConsoleHistory`: the console's history is kept in NVForever's folder.
    pub remember_console_history: bool,
    /// `bUseConsoleOutputFile`: console lines are added to a file in NVForever's folder.
    pub console_output_file: bool,
    /// `bConsoleTextShadow`: the console's scrollback darker, over a faint shadow.
    pub console_text_shadow: bool,
    /// `bClickToExitLoadScreens`: a loading screen waits for a press once all is in.
    pub click_to_exit_loading: bool,
    /// `bBarterShowCapsChange`: a barter shows each side's caps after the trade too.
    pub barter_caps_change: bool,
    /// `bJumpSwimsUpwards`: holding Jump under water swims up.
    pub jump_swims_up: bool,
    /// `bFirstPersonVATS`: V.A.T.S.'s firing plays in first person.
    pub first_person_vats: bool,
    /// `bKeepCrosshairWhenAiming`: the crosshair stays down the iron sights.
    pub crosshair_when_aiming: bool,
    /// `bMoveDuringVATSPlayback`: walking and running stay the player's while
    /// V.A.T.S.'s camera plays.
    pub move_during_kill_cam: bool,
    /// `bBetterPickupPrompt`: a stack's prompt gives the whole stack's weight and value.
    pub stack_figures: bool,
    /// `bTurnFurtherWhileSeated`: seated in first person the view turns 115 degrees to
    /// either side, not 80.
    pub seated_wide: bool,
    /// `bNoTurningAnim`: the player's body plays no turning-in-place clip.
    pub no_turn_clip: bool,
    /// `bRemoveLandingAnim`: the player's body plays no landing clip.
    pub no_land_clip: bool,
    /// `bHideInvisibleUndiscoveredLocations`: a place not found yet is on the compass only
    /// when the map shows it.
    pub hide_unmapped_places: bool,
    /// `bSkipVigorTesterSpecialPages`: the S.P.E.C.I.A.L. machine opens on its review
    /// page.
    pub vigor_review_first: bool,
    /// `bScopeVisibilityDelay`: milliseconds a scope waits after the zoom is full before
    /// it shows; 0 none.
    pub scope_delay_ms: f32,
    /// `bHideCursorInMessageMenu`: no mouse pointer over a message box.
    pub no_pointer_in_message_box: bool,
    /// `bHideCursorInDialog`: no mouse pointer over a conversation's choices.
    pub no_pointer_in_dialogue: bool,
    /// `bLevelUpScrollWheelSupport`: the wheel raises and lowers skills on the level-up
    /// page.
    pub levelup_wheel: bool,
    /// `bActivatingDoesntStandUp`: Activate does not get the player up from furniture;
    /// moving does.
    pub seat_keeps_on_activate: bool,
    /// `bNoExteriorLoadScreens`: leaving an interior shows a black screen with no
    /// picture or tip (not a loaded save).
    pub plain_going_out: bool,
    /// `bHideHotkeyedItemsInContainers`: items on a hotkey are not listed on the player's
    /// side of a container.
    pub hide_hotkeyed_containers: bool,
    /// `bHideHotkeyedItemsInBarter`: items on a hotkey are not offered in barter.
    pub hide_hotkeyed_barter: bool,
    /// `bHideHotkeyedItemsInRepairLists`: items on a hotkey are not offered as repair
    /// parts in the Pip-Boy.
    pub hide_hotkeyed_repair: bool,
    /// `bAddSetHomeCommand`: the console's `sethome`, `home`, `back` and `delhome`.
    pub console_homes: bool,
    /// `bShiftScreenshotHidesMenus`: with Shift held, a screenshot leaves out the HUD.
    pub shift_screenshot_hides: bool,
    /// `bExtraConsoleDetails`/`bMeshPath`: a selected reference's console details give
    /// its model.
    pub console_model_details: bool,
    /// `bMovementPenalties`: the pace walking or running backwards and sideways, as a
    /// part of the usual; 1 changes nothing.
    pub back_pace: f32,
    pub strafe_pace: f32,
    /// `bOverencumberedTweak`: over the carry limit the player may run and jump, paying
    /// `over_run_ap` Action Points a second running and `over_jump_ap` a jump.
    pub run_overencumbered: bool,
    pub over_run_ap: f32,
    pub over_jump_ap: f32,
    /// `bScopeHoldBreath`: Shift (or Activate when `breath_activate` is on) through a scope
    /// steadies it to `breath_sway` of its sway, paying `breath_ap` Action Points a second.
    pub hold_breath: bool,
    /// `[Hold Breath] bActivateHoldsBreath`: holding Activate also holds breath.
    pub breath_activate: bool,
    pub breath_sway: f32,
    pub breath_ap: f32,
    /// `bAdjustableScopeZoom`: the wheel zooms a scope in and out.
    pub scope_wheel_zoom: bool,
    /// `bMousewheelScrollsWeaponHotkeys`: in first person the mouse wheel steps through
    /// the hotkeyed weapons.
    pub wheel_cycles_hotkeys: bool,
    /// `bWeaponCycleUpDownHotkeys`: with a controller, the D-pad's left and right step
    /// through the hotkeyed weapons (instead of Hotkey 4 and Ammo Swap).
    pub dpad_cycles_hotkeys: bool,
    /// `bAllowKeyboardAndMouseWithControllerConnected`: with the controller as the input
    /// device, the keyboard and mouse still move, look and act (not in menus).
    pub keys_with_pad: bool,
    /// `bDoubleTapReloadToChangeAmmoType`: a second Ready Item tap swaps the ammo; a single
    /// tap reloads after a short wait for it.
    pub double_tap_ammo: bool,
    /// `bImprovedWeather` (`fFastTravelWeatherChangeDistanceThreshold`): a fast travel
    /// shorter than this many units keeps the weather.
    pub improved_weather: bool,
    /// `bVATSStopFollowingDeadTargets`: a kill camera stops following its target once it
    /// is dead.
    pub vats_stop_following_dead: bool,
    /// `bAddNightVisionToggle` (`bDisableVisionByDefault`, `bAllowNightVisionDuringDay`):
    /// the light's key switches a night-vision scope's night vision; off until switched
    /// on; by day too.
    pub night_vision_toggle: bool,
    pub night_vision_off_first: bool,
    pub night_vision_by_day: bool,
    /// A kill camera for the player's kills outside VATS mode (ours; Stewie's
    /// `bUseHitLocationInCinematicKillcams` speaks of that camera), and
    /// `bUseHitLocationInCinematicKillcams`: kill cameras look at the part struck.
    pub kill_cam_outside_vats: bool,
    pub kill_cam_hit_location: bool,
    pub weather_trip_distance: f32,
    /// `bCustomLocationDiscoveredSound` (`sEditorID`): the sound a discovered place plays,
    /// by Editor ID; empty: the game's own.
    pub discovered_sound: String,
    /// `bAlternateLevelupSounds` (`iLevelA`, `sSoundA`, `iLevelB`, `sSoundB`): every so many
    /// levels the level-up plays this sound instead of its own (0 or empty: never).
    pub level_sound_every_a: f32,
    pub level_sound_a: String,
    pub level_sound_every_b: f32,
    pub level_sound_b: String,
    /// `bCompassNPCDistanceBasedAlpha`: actors' compass pips fade with distance over the
    /// sneak range.
    pub compass_pips_fade: bool,
    /// `bCompassLocationDistanceBasedAlpha`: places on the compass fade with distance
    /// over the reach they show at.
    pub compass_places_fade: bool,
    /// `bRecentlyDeadNPCIndicator`: seconds the newly dead stay on the compass, until the
    /// crosshair has been on them; 0 none.
    pub recent_dead_secs: f32,
    /// `bArmorSoundsPlayIn3D`: in third person the player's armour and clothing rustle
    /// from the body.
    pub armor_sounds_3d: bool,
    /// `bEncumbranceIncludesGrabbedItems`: an item held up by the grab counts in the
    /// carried weight.
    pub grabbed_weighs: bool,
    /// `bSortEquipableAmmo`: in the Pip-Boy the ammo the carried weapons fire comes first.
    pub usable_ammo_first: bool,
    /// `bBetterFlycam`: the free camera flies up and down on Space and Ctrl, on by
    /// itself with Q, and turns with Z and C.
    pub better_flycam: bool,
    /// `iRepeatActivateKey`/`bOnlyTakeItems`: the Repeat Activate key takes only items.
    pub repeat_activate_takes_only: bool,
    /// `bForceHiResWeaponModels`: NPCs hold their weapons' first-person models.
    pub hi_res_npc_weapons: bool,
    /// `bShowQuestObjectivesOnCellChange`/`fMinInterval`: a new place shows the active
    /// quest's open objectives, at most once in `objectives_gap` seconds.
    pub objectives_on_arrival: bool,
    pub objectives_gap: f32,
    /// `bClearNearbyPlayerMarker`/`fDistance`: the custom map marker goes when the player
    /// comes this near it (game units; 0 never).
    pub clear_marker_within: f32,
    /// `iPlaceMapMarkerAtPlayerPosKey`/`sMessage`: the Place Marker Here key says so in
    /// the corner.
    pub place_marker_here_says: bool,
    /// `bKeepFallHeightOnLoad`: a save made in the air keeps the fall's top.
    pub keep_fall_height_on_load: bool,
    /// `bRememberPipboyScrollPositions`: the Pip-Boy's item lists open where they were
    /// left, kept with the save ([`ItemPlaces`]).
    pub remember_item_places: bool,
    /// `bHUDMarkerNameIndicator`: the name of the map marker the player faces, over the
    /// compass; within `marker_name_angle` degrees, places not found left out with
    /// `marker_name_hide_unfound`, its distance in `marker_name_units` (0 none, 1 metres,
    /// 2 feet).
    pub hud_marker_name: bool,
    /// `bHUDShowRegionNames`: the region the player stands in, named over the compass.
    pub hud_region_names: bool,
    /// `bMoveDuringOpenPipboyAnim`: the player moves on while the Pip-Boy comes up.
    pub move_during_pipboy_raise: bool,
    /// `bDisableRegionBordersKeepMessage`: world borders warn but do not stop the player.
    pub region_borders_message_only: bool,
    pub marker_name_angle: f32,
    pub marker_name_hide_unfound: bool,
    pub marker_name_units: u8,
    /// `iHideHUDKey`: what the Hide HUD key hides: the health and compass piece, the
    /// action points and ammo piece, the Hardcore needs words and the sneak word.
    pub hide_hud_compass: bool,
    pub hide_hud_ammo: bool,
    pub hide_hud_needs: bool,
    pub hide_hud_sneak: bool,
    /// `iHideHUDKey` `bHideByDefault`: a loaded save starts with those parts hidden.
    pub hide_hud_on_load: bool,
    /// `bTerminalGreyReadNotes`: terminal entries whose note was read are drawn dimmer.
    pub grey_read_notes: bool,
    /// `bScaleWeaponVolume`: the loudness of the player's shot sounds, a percentage.
    pub player_weapon_volume: f32,
    /// `bScaleWeaponVolume`: the loudness of other actors' shot sounds, a percentage.
    pub npc_weapon_volume: f32,
    /// `bCompassQuestHeightIndicator`: a quest mark well above or below the player has a
    /// bar over or under it on the compass.
    pub compass_quest_height: bool,
    /// `bPlacedMarkerHeightIndicator`: the same for the player's custom map marker.
    pub compass_marker_height: bool,
    /// `bRememberWeaponAmmos` [documented: lStewieAl's Tweaks 10.05
    /// (`https://www.nexusmods.com/newvegas/mods/66347`) `nvse_stewie_tweaks.ini`
    /// `[Tweaks]`: `; remember ammo type and count for all player weapons` /
    /// `bRememberWeaponAmmos = 0`; no DLL or source consulted]: remember the player's
    /// last chosen ammo type per weapon base form across switching, unequipping,
    /// dropping and saving/loading (#3305, #3877) [design: remembers chosen ammo type
    /// per weapon base form while magazine ammo counts are already tracked per weapon
    /// via `magazine_ammo`].
    pub keep_chosen_ammo: bool,
}

/// The colours a companion's compass pip may take, ours; the first leaves it as anyone's.
pub const COMPANION_COLOURS: [(&str, [f32; 3]); 6] = [("Off", [0.0; 3]), ("Green", [0.35, 1.0, 0.4]), ("Blue", [0.4, 0.6, 1.0]), ("Cyan", [0.3, 0.95, 1.0]), ("White", [1.0, 1.0, 1.0]), ("Yellow", [1.0, 0.95, 0.3])];

/// The companion pip colour the tweak picks, if any.
pub fn companion_colour(t: Option<&TweakSettings>) -> Option<Color> {
    let i = t.map_or(0, |t| t.companion_pip).min(COMPANION_COLOURS.len() - 1);
    (i > 0).then(|| {
        let [r, g, b] = COMPANION_COLOURS[i].1;
        Color::srgb(r, g, b)
    })
}


/// The walk-angle choices: 30 to 80 degrees by 5; 45 is the physics' own.
const ANGLES: usize = 11;
fn walk_angle(i: usize) -> f32 {
    30.0 + 5.0 * i as f32
}

/// The Own Skill Points Rule's numbers until the player sets them: the game's rule
/// without its odd-level point (10 + Intelligence / 2) [design].
const SKILL_POINTS_START: (i32, f64) = (10, 0.5);
const SKILL_BASES: usize = 31;

impl TweakSettings {
    /// Hand the skill-point numbers to the rule while it is on.
    fn apply_skill_points(&mut self) {
        if self.game.skill_points.is_some() {
            self.game.skill_points = Some((self.skill_point_base, self.skill_points_per_int));
        }
    }

    /// Label shown for the pause menu's Console entry (`[Help Button Replacer] sButtonLabel`
    /// [documented: Stewie Tweaks v10.05 Readme & generated `nvse_stewie_tweaks.ini`]),
    /// falling back to `"Console"` when empty or whitespace-only [design].
    pub fn console_label(&self) -> &str {
        let trimmed = self.help_console_label.trim();
        if trimmed.is_empty() { "Console" } else { trimmed }
    }
}

/// `NVForever.ini` `[Settings]` key supplying custom text for the pause menu's Console entry
/// (`[Help Button Replacer] sButtonLabel` [documented: Stewie Tweaks v10.05 Readme & generated
/// `nvse_stewie_tweaks.ini`]) [design: no free-text widget on the Tweaks tab; kept in
/// `NVForever.ini` alongside `tweak-help-is-console`, never imported from
/// `nvse_stewie_tweaks.ini`].
pub const HELP_CONSOLE_LABEL_KEY: &str = "tweak-help-console-label";

/// Read `[Settings] tweak-help-console-label` from `NVForever.ini` (`SettingsFile`) at startup
/// into [`TweakSettings::help_console_label`] [design]. Unattended runs in the player's own
/// home (`SettingsFile(None)`) leave the label at its default.
pub(crate) fn read_help_console_label(file: Option<Res<crate::settings::SettingsFile>>, mut tweaks: ResMut<TweakSettings>) {
    let Some(path) = file.and_then(|f| f.0.clone()) else { return };
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    if let Some(label) = parse_help_console_label(&text) {
        info!("tweaks: [{}] {HELP_CONSOLE_LABEL_KEY}={label} read from {}", crate::settings::SECTION, path.display());
        tweaks.help_console_label = label;
    }
}

fn parse_help_console_label(text: &str) -> Option<String> {
    let raw = fnv_install::ini_get(text, crate::settings::SECTION, HELP_CONSOLE_LABEL_KEY)?;
    let trimmed = raw.trim();
    let unquoted = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(trimmed).trim();
    (!unquoted.is_empty()).then(|| unquoted.to_string())
}

/// Action Points per second drained while sneak-running under `[Running Costs AP]`:
/// `fSneakAPDrainCostBase + fSneakRunAPEnduranceMult * (Endurance - 5)`
/// [documented: Stewie Tweaks v10.05 generated `nvse_stewie_tweaks.ini` `[Running Costs AP]`
/// comment; option names also in v10.05 Readme & `AllTweaks.json`; hypothesis: sign vs. the
/// v10.05 Readme's `(5 - Endurance)` standing-run formula is unobserved in the original
/// binary].
///
/// [design]: returns `0.0` whenever `base <= 0.0` (disabled) and clamps the computed drain
/// rate at `0.0` (never negative).
pub fn sneak_run_ap_rate(base: f32, endurance_mult: f32, endurance: f32) -> f32 {
    if base <= 0.0 {
        return 0.0;
    }
    (base + endurance_mult * (endurance - 5.0)).max(0.0)
}

/// Whether `actor` wears power armour on the upper body.
pub fn wears_power_armor(world: &fnv_sim::World, actor: fnv_sim::FormId) -> bool {
    use fnv_sim::forms::ItemKind;
    world.equipment(actor).unwrap_or_default().into_iter().filter_map(|i| world.forms.item(i)).any(|it| it.kind == ItemKind::Armor && it.power_armor && it.slots.is_some_and(|s| s & fnv_game::detection::UPPER_BODY != 0))
}

/// The perks-per-level choices (1 to 5) and the S.P.E.C.I.A.L. totals (30 to 70 by 2).
const PERKS: usize = 5;
const PERK_CHOICES: [&str; PERKS] = ["1", "2", "3", "4", "5"];
/// Place Name: Distance choices (0 none, 1 metres, 2 feet).
const MARKER_NAME_UNITS: [&str; 3] = ["None", "Metres", "Feet"];
const SPECIAL_LOW: i32 = 30;
const SPECIAL_STEP: i32 = 2;
const SPECIALS: usize = 21;

fn choice_names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

fn take_perks_per_level(world: &mut World, text: &str) -> bool {
    let names = choice_names(&PERK_CHOICES);
    let Some(i) = choice_of(text, &names, |v| choice_index(v, PERKS)) else { return false };
    if let Some(mut t) = world.get_resource_mut::<TweakSettings>() {
        t.game.perks_per_level = i + 1;
        true
    } else {
        false
    }
}

fn take_date_style(world: &mut World, text: &str) -> bool {
    let names = choice_names(&crate::wait::model::DATE_STYLES);
    let Some(i) = choice_of(text, &names, |v| choice_index(v, crate::wait::model::DATE_STYLES.len())) else { return false };
    if let Some(mut t) = world.get_resource_mut::<TweakSettings>() {
        t.date_style = i;
        true
    } else {
        false
    }
}

fn take_companion_pip(world: &mut World, text: &str) -> bool {
    let names: Vec<String> = COMPANION_COLOURS.iter().map(|(n, _)| (*n).to_string()).collect();
    let Some(i) = choice_of(text, &names, |v| choice_index(v, COMPANION_COLOURS.len())) else { return false };
    if let Some(mut t) = world.get_resource_mut::<TweakSettings>() {
        t.companion_pip = i;
        true
    } else {
        false
    }
}

fn take_marker_name_units(world: &mut World, text: &str) -> bool {
    let names = choice_names(&MARKER_NAME_UNITS);
    let Some(i) = choice_of(text, &names, |v| choice_index(v, MARKER_NAME_UNITS.len())) else { return false };
    if let Some(mut t) = world.get_resource_mut::<TweakSettings>() {
        t.marker_name_units = i as u8;
        true
    } else {
        false
    }
}

impl Default for TweakSettings {
    fn default() -> TweakSettings {
        TweakSettings {
            game: Tweaks::default(),
            no_karma_messages: false,
            no_xp_messages: false,
            no_quest_failed_popup: false,
            no_location_popup: false,
            no_objective_popup: false,
            disable_fast_travel: false,
            midair_fast_travel: false,
            fast_travel_enemies_nearby: false,
            fast_travel_overencumbered: false,
            no_red_crosshair: false,
            hide_enemy_markers: false,
            no_combat_music: false,
            no_loading_tips: false,
            no_reputation_messages: false,
            no_explosion_blur: false,
            no_sneak_label: false,
            kill_cam_exit_key: false,
            numbered_terminal_keys: false,
            deselect_hotkeys: false,
            dialogue_music_volume: 1.0,
            dialogue_radio_volume: 1.0,
            console_skips_player: false,
            holster_real_time: false,
            power_armor_no_screen_blood: false,
            no_red_crosshair_on_cloaked: false,
            god_mode_quiet_landing: false,
            console_compress_repeats: false,
            console_timestamps: false,
            living_anatomy_dr: false,
            no_travel_time: false,
            power_armor_fall_damage: 1.0,
            no_travel_crippled_leg: false,
            jump_ap_cost: 0.0,
            jump_ap_danger_only: false,
            jump_without_enough_ap: false,
            run_ap_per_second: 0.0,
            sneak_run_ap_base: 0.0,
            sneak_run_ap_endurance_mult: 0.0,
            no_selling: false,
            skill_point_base: SKILL_POINTS_START.0,
            skill_points_per_int: SKILL_POINTS_START.1,
            no_aiming_slowdown: false,
            repeat_jumping: false,
            hold_to_sneak: false,
            power_attack_ap: 0.0,
            quest_marker_reach: 0.0,
            scope_zoom_time: 1.0,
            drop_key: false,
            aiming_draws_weapon: false,
            walk_angle: fnv_phys::CharacterParams::default().max_slope_deg,
            crippled_leg_jump: 1.0,
            no_player_stagger: false,
            ready_does_not_draw: false,
            no_hotkeys_while_reloading: false,
            wading_pace: 1.0,
            min_splatter_damage: 0.0,
            xp_sound_at_cap: false,
            hold_to_wait: false,
            no_sight_zoom: false,
            hide_misc_quest: false,
            tab_click_sorts: false,
            hotkey_toggles_in_hand: false,
            no_interior_fog: false,
            compass_pips_in_danger_only: false,
            neutral_foes_on_compass: false,
            sneak_label_standing: false,
            no_xp_meter_in_combat: false,
            no_limb_indicator: false,
            skip_death_cam: false,
            skip_death_cam_attack: 0.0,
            help_is_console: false,
            help_console_label: String::new(),
            heartbeat_fade: false,
            heartbeat_fade_seconds: 5.0,
            terminal_fade: false,
            terminal_fade_seconds: 1.0,
            containers_show_ammo: false,
            descriptive_marker_added: false,
            no_grenade_indicator: false,
            grenade_indicator_skip_harmless: false,
            grenade_indicator_destructibles: false,
            take_all_stays: false,
            take_all_confirm: false,
            take_all_confirm_min: 2.0,
            repair_all_confirm: false,
            repair_all_threshold: 0.0,
            hide_equipped_barter: false,
            hide_equipped_containers: false,
            title_case_categories: false,
            no_right_click_drop: false,
            quiet_undiscovered: false,
            crosshair_hidden: false,
            aim_turn_x: 1.0,
            aim_turn_y: 1.0,
            radio_music_volume: 1.0,
            radio_talk_volume: 1.0,
            camera_wheel_needs_pov: false,
            quiet_hotkeys: false,
            no_weapon_hotkeys: false,
            pipboy_opens_items: false,
            hide_crosshair_reloading: false,
            invert_x: false,
            nearest_unfound_on_compass: false,
            discovered_in_corner: false,
            invert_title_wheel: false,
            auto_continue: false,
            continue_shows_save: false,
            quest_text_in_menus: false,
            detected_by_whom: false,
            detected_names: 2.0,
            key_repeat_accel: false,
            hud_fatigue: false,
            toggle_device: false,
            scale_explosion_shake: false,
            explosion_shake: 1.0,
            key_repeat_delay: 1.0,
            key_repeat_mult: 2.0,
            key_repeat_delay_alt: 2.5,
            key_repeat_mult_alt: 4.0,
            precise_rad_meter: false,
            rad_decimals: 2,
            compass_doors: false,
            compass_doors_exterior: 2000.0,
            compass_doors_interior: 1000.0,
            compass_doors_fade: false,
            armor_visible_below: 0.0,
            armor_red_below: 0.25,
            armor_flash_below: 0.1,
            armor_in_weapon_place: true,
            hide_ammo_label: false,
            free_item_price: 0.0,
            barter_by_kind: false,
            barter_water_unscaled: false,
            barter_sell_armor: 1.0,
            barter_sell_weapon: 1.0,
            barter_sell_aid: 1.0,
            barter_sell_ammo: 1.0,
            barter_sell_misc: 1.0,
            barter_sell_weapon_mod: 1.0,
            barter_buy_armor: 1.0,
            barter_buy_weapon: 1.0,
            barter_buy_aid: 1.0,
            barter_buy_ammo: 1.0,
            barter_buy_misc: 1.0,
            barter_buy_weapon_mod: 1.0,
            duplicate_binds: false,
            pov_follows_weapon: false,
            kill_cam_chance: 1.0,
            warn_encumbrance: false,
            pipboy_buzz: false,
            power_attack_if_blocking: false,
            vats_hip_fire: false,
            no_pipboy_in_combat: false,
            no_weapon_move_penalty: false,
            unequip_broken_armor: false,
            hide_crosshair_first: false,
            hide_crosshair_third: false,
            red_hp_below: 0.0,
            red_ap_below: 0.0,
            weapon_cnd_red_below: 0.0,
            weapon_cnd_flash_below: 0.0,
            blackjack_total: false,
            slots_hold_to_spin: false,
            shift_talks_sneaking: false,
            tab_closes_terminals: false,
            tab_back_in_pause: false,
            date_style: 0,
            vats_keeps_holstered: false,
            no_hotkey_wheel: false,
            ammo_label_under: false,
            companion_pip: 0,
            female_facial_hair: false,
            store_all: false,
            quick_load_in_menus: false,
            pick_locks_with_key: false,
            smooth_look: false,
            right_click_previous_category: false,
            full_barter_total: false,
            compass_fade_left: false,
            custom_dead_zones: false,
            left_dead_zone: 0.0,
            right_dead_zone: 0.0,
            custom_trigger_dead_zones: false,
            lt_dead_zone: 0.75,
            rt_dead_zone: 0.75,
            responsive_stick: false,
            faster_pad_orbit: false,
            vanity_returns_first: false,
            horizontal_look: 1.0,
            remember_console_history: false,
            console_output_file: false,
            console_text_shadow: false,
            click_to_exit_loading: false,
            barter_caps_change: false,
            jump_swims_up: false,
            first_person_vats: false,
            crosshair_when_aiming: false,
            move_during_kill_cam: false,
            stack_figures: false,
            seated_wide: false,
            no_turn_clip: false,
            no_land_clip: false,
            hide_unmapped_places: false,
            vigor_review_first: false,
            scope_delay_ms: 0.0,
            no_pointer_in_message_box: false,
            no_pointer_in_dialogue: false,
            levelup_wheel: false,
            seat_keeps_on_activate: false,
            plain_going_out: false,
            hide_hotkeyed_containers: false,
            hide_hotkeyed_barter: false,
            hide_hotkeyed_repair: false,
            console_homes: false,
            shift_screenshot_hides: false,
            console_model_details: false,
            back_pace: 1.0,
            strafe_pace: 1.0,
            run_overencumbered: false,
            over_run_ap: 4.0,
            over_jump_ap: 10.0,
            hold_breath: false,
            breath_activate: false,
            breath_sway: 0.2,
            breath_ap: 5.0,
            scope_wheel_zoom: false,
            wheel_cycles_hotkeys: false,
            dpad_cycles_hotkeys: false,
            keys_with_pad: false,
            double_tap_ammo: false,
            improved_weather: false,
            vats_stop_following_dead: false,
            night_vision_toggle: false,
            night_vision_off_first: false,
            night_vision_by_day: false,
            kill_cam_outside_vats: false,
            kill_cam_hit_location: false,
            weather_trip_distance: 30000.0,
            discovered_sound: String::new(),
            level_sound_every_a: 0.0,
            level_sound_a: String::new(),
            level_sound_every_b: 0.0,
            level_sound_b: String::new(),
            compass_pips_fade: false,
            compass_places_fade: false,
            recent_dead_secs: 0.0,
            armor_sounds_3d: false,
            grabbed_weighs: false,
            usable_ammo_first: false,
            better_flycam: false,
            repeat_activate_takes_only: true,
            hi_res_npc_weapons: false,
            objectives_on_arrival: false,
            objectives_gap: 30.0,
            clear_marker_within: 0.0,
            place_marker_here_says: true,
            keep_fall_height_on_load: false,
            remember_item_places: false,
            grey_read_notes: false,
            hud_marker_name: false,
            hud_region_names: false,
            move_during_pipboy_raise: false,
            region_borders_message_only: false,
            marker_name_angle: 10.0,
            marker_name_hide_unfound: true,
            marker_name_units: 0,
            hide_hud_compass: true,
            hide_hud_ammo: true,
            hide_hud_needs: true,
            hide_hud_sneak: true,
            hide_hud_on_load: false,
            player_weapon_volume: 100.0,
            npc_weapon_volume: 100.0,
            compass_quest_height: false,
            compass_marker_height: false,
            keep_chosen_ammo: false,
        }
    }
}

/// The value of a tweak, for modules that only have the world.
pub fn tweaks(world: &World) -> TweakSettings {
    world.get_resource::<TweakSettings>().cloned().unwrap_or_default()
}

/// An on/off row of the Tweaks tab for the field `$($f).+` of [`TweakSettings`].
macro_rules! switch {
    ($app:expr, $key:literal, $label:literal, $note:literal, $($f:ident).+) => {
        add_setting(
            $app,
            Setting {
                key: concat!("tweak-", $key),
                group: GROUP,
                label: $label,
                note: $note,
                switches: "",
                get: |w| w.get_resource::<TweakSettings>().is_some_and(|t| t.$($f).+),
                set: |w, on| {
                    if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                        t.$($f).+ = on;
                    }
                },
                slider: None,
            },
        )
    };
}

/// The S.P.E.C.I.A.L. total of choice `i`, and back.
fn special_total(i: usize) -> i32 {
    SPECIAL_LOW + SPECIAL_STEP * i as i32
}

fn special_index(total: i32) -> usize {
    (((total - SPECIAL_LOW) / SPECIAL_STEP).max(0) as usize).min(SPECIALS - 1)
}

/// A multiplier as a slider: the part `v` is the factor `2v` (0 to 2 in tenths, 1 in the
/// middle).
fn factor(v: f32) -> f64 {
    f64::from(v) * 2.0
}

fn part(f: f64) -> f32 {
    (f / 2.0).clamp(0.0, 1.0) as f32
}

fn shown_factor(v: f32) -> String {
    format!("x{:.1}", factor(v))
}

/// An on/off row for one `WEIGHTLESS_*` bit of the game's `weightless_kinds`.
macro_rules! weightless {
    ($app:expr, $key:literal, $label:literal, $note:literal, $bit:expr) => {
        add_setting(
            $app,
            Setting {
                key: concat!("tweak-", $key),
                group: GROUP,
                label: $label,
                note: $note,
                switches: "",
                get: |w| w.get_resource::<TweakSettings>().is_some_and(|t| t.game.weightless_kinds & $bit != 0),
                set: |w, on| {
                    if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                        t.game.weightless_kinds = if on { t.game.weightless_kinds | $bit } else { t.game.weightless_kinds & !$bit };
                    }
                },
                slider: None,
            },
        )
    };
}

/// A slider row for an amount from 0 to `$max` in twentieths, shown as a whole number,
/// in the field `$($f).+` (of type `$t`).
macro_rules! amount {
    ($app:expr, $key:literal, $label:literal, $note:literal, $max:expr, $t:ty, $($f:ident).+) => {
        add_setting(
            $app,
            Setting::slider(
                concat!("tweak-", $key),
                GROUP,
                $label,
                $note,
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| (t.$($f).+ as f32 / $max as f32).clamp(0.0, 1.0)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.$($f).+ = (v * $max as f32).round() as $t;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.0}", v * $max as f32)),
                },
            ),
        )
    };
}

/// A list row of the Tweaks tab choosing a sound by Editor ID ([`sound_names`]) for the
/// `String` field `$f` of [`TweakSettings`] (empty: the game's own).
macro_rules! sound_list {
    ($app:expr, $key:literal, $label:literal, $note:literal, $f:ident) => {{
        crate::settings::add_choice_list(
            $app,
            Setting { key: concat!("tweak-", $key), group: GROUP, label: $label, note: $note, switches: "", get: |_| false, set: |_, _| {}, slider: None },
            crate::settings::ChoiceList {
                names: sound_names,
                chosen: |w| w.get_resource::<TweakSettings>().map(|t| t.$f.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| GAME_SOUND.to_string()),
                pick: |w, name| w.get_resource_mut::<TweakSettings>().map(|mut t| t.$f = sound_kept(name)).is_some(),
                save: |w| w.get_resource::<TweakSettings>().map(|t| t.$f.clone()).unwrap_or_default(),
                load: |w, name| w.get_resource_mut::<TweakSettings>().map(|mut t| t.$f = sound_kept(name)).is_some(),
                ask_keep: false,
            },
        );
        // Strip the redundant `UI` prefix when displaying sound choices so vanilla Editor
        // IDs such as `UILockpickingCylinderSqueakA` fit the value box without truncation,
        // while keeping full Editor IDs in storage and INI files [design].
        crate::settings::set_choice_labels($app, concat!("tweak-", $key), |_, name| {
            name.strip_prefix("UI").filter(|s| !s.is_empty()).unwrap_or(name).to_string()
        });
    }};
}

/// A slider row of the Tweaks tab for a part (0 to 100% in fives) in the field
/// `$($f).+` of [`TweakSettings`].
macro_rules! part {
    ($app:expr, $key:literal, $label:literal, $note:literal, $($f:ident).+) => {
        add_setting(
            $app,
            Setting::slider(
                concat!("tweak-", $key),
                GROUP,
                $label,
                $note,
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(1.0, |t| t.$($f).+),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.$($f).+ = v;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: None,
                },
            ),
        )
    };
}

pub struct TweaksPlugin;

impl Plugin for TweaksPlugin {
    fn build(&self, app: &mut App) {
        crate::save::add_save_hook(app, ITEM_PLACES_SAVE_HOOK);
        crate::save::add_save_hook(app, HIDE_HUD_SAVE_HOOK);
        app.init_resource::<TweakSettings>()
            .init_resource::<ItemPlaces>()
            .add_systems(Startup, read_help_console_label)
            .add_systems(PostStartup, apply_modular_stewie_inis.before(crate::settings::apply_stored))
            .add_systems(Update, remember_item_places.after(crate::menu::MenuSystems))
            .add_systems(Update, sync.in_set(crate::PlaySet::Input)
                .before(crate::player::read_input).before(crate::control::read_controls))
            .add_systems(Update, (keys, drop_weapon, radio_volume_keys, last_weapon, hold_breath, repeat_activate, toggle_sight, place_marker_here, clear_near_marker, hide_hud_key, skip_radio_song).in_set(crate::PlaySet::Input))
            .add_systems(Update, (objectives_on_arrival, revive_companions_on_arrival).in_set(crate::PlaySet::Ui));
        #[cfg(not(target_os = "android"))]
        app.add_systems(Update, toggle_device.in_set(crate::PlaySet::Input));
        app
            // On Android, Switch Device by Pressing is admitted by pad::device::automatic,
            // through the same consumed edge/cancellation/save path as the selector.
            .add_systems(PreUpdate, trigger_dead_zones.before(bevy::input::InputSystems))
            .add_systems(Update, ap_costs.after(crate::PlaySet::Input).after(crate::player::encumbered_walks).before(crate::PlaySet::Physics));
        sound_list!(app, "discovered-sound", "Location Discovered Sound", "The sound a newly discovered place plays: the game's own, or another of its interface sounds. (Stewie's Tweaks bCustomLocationDiscoveredSound)", discovered_sound);
        // Named off-by-default NVForever option (#3876, Gameplay > Game, docs/spec-notes/npc-weapon-mods.md;
        // follows JIP LN NVSE Plugin 57.30 jip_nvse.ini [GamePatches] bNPCWeaponMods, never reads jip_nvse.ini).
        add_setting(
            app,
            Setting {
                key: "npc-weapon-mods",
                group: crate::pause::GROUP_MENUS,
                label: "NPC weapon mods",
                note: "On: NPCs have a level-scaled chance (10%-60%, +2%/level) for their main weapon to spawn with random weapon mods. Off: NPC weapons have no extra mods, as in the original.",
                switches: "",
                get: |w| w.get_resource::<TweakSettings>().is_some_and(|t| t.game.npc_weapon_mods),
                set: |w, on| {
                    if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                        t.game.npc_weapon_mods = on;
                    }
                },
                slider: None,
            },
        );
        amount!(app, "level-sound-every-a", "Level-Up Sound A: Every N Levels", "Every this many levels the level-up plays Sound A instead of its own; 0 never. (Stewie's Tweaks bAlternateLevelupSounds, iLevelA)", 50.0, f32, level_sound_every_a);
        sound_list!(app, "level-sound-a", "Level-Up Sound A", "The sound for those levels. (Stewie's Tweaks bAlternateLevelupSounds, sSoundA)", level_sound_a);
        amount!(app, "level-sound-every-b", "Level-Up Sound B: Every N Levels", "Sound B every this many levels; where both fall on one level, the larger wins. 0 never. (Stewie's Tweaks bAlternateLevelupSounds, iLevelB)", 50.0, f32, level_sound_every_b);
        sound_list!(app, "level-sound-b", "Level-Up Sound B", "The sound for those levels. (Stewie's Tweaks bAlternateLevelupSounds, sSoundB)", level_sound_b);
        // ---- experience ----
        add_setting(
            app,
            Setting::slider(
                "tweak-xp-multiplier",
                GROUP,
                "Experience Earned",
                "Every experience gain is multiplied by this, after the perks; x1.0 changes nothing. (Stewie's Tweaks bReduceXP)",
                Slider {
                    get: |w| part(w.get_resource::<TweakSettings>().map_or(1.0, |t| t.game.xp_multiplier)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.xp_multiplier = factor(v);
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(shown_factor),
                },
            ),
        );
        switch!(app, "no-xp-messages", "Quiet Experience", "Experience gains do not bring up the XP meter or its sound; levelling up still does. (Stewie's Tweaks bNoXPMessages)", no_xp_messages);
        // ---- messages ----
        switch!(app, "no-karma-messages", "Quiet Karma", "Karma changes show no corner message and play no sound. (Stewie's Tweaks bNoKarmaMessages)", no_karma_messages);
        switch!(app, "no-quest-failed", "No Quest Failed Banner", "A failed quest puts up no banner; the Pip-Boy still lists it. (Stewie's Tweaks bNoQuestFailedPopup)", no_quest_failed_popup);
        switch!(app, "no-location-popup", "No Location Banner", "Discovering a place puts up no banner; it is still marked on the map. (Stewie's Tweaks bNoLocationPopup)", no_location_popup);
        switch!(app, "no-objective-popup", "No Objective List", "New and completed quest objectives are not listed on the HUD. (Stewie's Tweaks bRemoveQuestObjectiveAddedText)", no_objective_popup);
        // ---- fast travel ----
        switch!(app, "disable-fast-travel", "No Fast Travel", "Fast travel is always refused. (Stewie's Tweaks bDisableFastTravel)", disable_fast_travel);
        switch!(app, "midair-fast-travel", "Fast Travel in the Air", "Being in the air does not stop fast travel. (Stewie's Tweaks bMidairFastTravel)", midair_fast_travel);
        switch!(app, "fast-travel-enemies", "Fast Travel Near Enemies", "Enemies nearby do not stop fast travel. (Stewie's Tweaks bFastTravelWithEnemiesNearby)", fast_travel_enemies_nearby);
        switch!(app, "fast-travel-encumbered", "Fast Travel Encumbered", "Carrying too much does not stop fast travel, as with Long Haul. (Stewie's Tweaks bFastTravelOverencumbered)", fast_travel_overencumbered);
        // ---- HUD ----
        switch!(app, "no-red-crosshair", "No Red Crosshair", "The crosshair keeps its colour over hostile actors. (Stewie's Tweaks bNoRedCrosshairOnEnemies)", no_red_crosshair);
        switch!(app, "hide-enemy-markers", "No Enemies on the Compass", "Hostile actors are left off the compass. (Stewie's Tweaks bHideEnemyMarkers)", hide_enemy_markers);
        switch!(app, "no-sneak-label", "No Sneak Label", "Sneaking shows no Hidden / Detected label on the HUD. (Stewie's Tweaks bRemoveSneakLabel)", no_sneak_label);
        switch!(app, "no-reputation-messages", "Quiet Reputation", "Reputation changes put up no box and play no sound; the Pip-Boy still shows them. (Stewie's Tweaks bNoReputationMessages)", no_reputation_messages);
        // ---- sound and screen ----
        switch!(app, "no-combat-music", "No Battle Music", "Fights do not switch the music to battle tracks. (Stewie's Tweaks bDisableCombatMusic)", no_combat_music);
        switch!(app, "no-loading-tips", "No Loading Tips", "Loading screens show their picture without the tip. (Stewie's Tweaks bDisableLoadingScreenTips)", no_loading_tips);
        switch!(app, "no-explosion-blur", "No Explosion Blur", "A blast close by does not blur the screen. (Stewie's Tweaks bDisableExplosionInFaceIMOD)", no_explosion_blur);
        switch!(app, "no-chem-wore-off", "No Chem Wore-Off Effect", "A chem wearing off plays no screen effect. (Stewie's Tweaks bRemoveChemWarnOffIMOD)", game.no_chem_wore_off_effect);
        // ---- VATS ----
        switch!(app, "kill-cam-exit-key", "Skip Kill Camera", "A right click ends a kill camera at once. (Stewie's Tweaks bVatsExitKey)", kill_cam_exit_key);
        // ---- menus ----
        switch!(app, "skip-skill-page", "Perks First When No Points", "A level-up with no skill points to spend opens on the perks. (Stewie's Tweaks bSkipSkillMenuIfNoPointsToAssign)", game.skip_skill_page_without_points);
        switch!(app, "no-empty-book-message", "No Message for Spent Books", "A skill book that gives no points (the skill at its top) shows no message. (Stewie's Tweaks bNoSkillMessageIfIncreaseIsZero)", game.no_book_message_without_gain);
        switch!(app, "recipe-caps", "Recipe Categories in Capitals", "The crafting menu's categories are shown in capitals. (Stewie's Tweaks bCapitalizeRecipeCategories)", game.capitalize_recipe_categories);
        switch!(app, "weapon-ammo-use", "Rounds per Shot", "A weapon that uses more than one round a shot says how many beside its ammo in the Pip-Boy. (Stewie's Tweaks bShowWeaponAmmoUseInMenus)", game.show_weapon_ammo_use);
        switch!(app, "book-effects", "Skill Book Effects", "A skill book says which skill it raises and by how much in the Pip-Boy. (Stewie's Tweaks bShowBookEffects)", game.show_book_effects);
        switch!(app, "deselect-hotkeys", "Unassign Hotkeys", "Assigning an item to the hotkey it already has takes it off. (Stewie's Tweaks bDeselectHotkeys)", deselect_hotkeys);
        switch!(app, "map-remembers", "Map Remembers Its Place", "The Pip-Boy maps open where they were left. (Stewie's Tweaks bMapRemembersPosition)", game.remember_map_position);
        switch!(app, "terminal-number-keys", "Number Keys on Terminals", "1 to 9 and 0 choose a terminal's rows. (Stewie's Tweaks bNumberedComputerHotkeys)", numbered_terminal_keys);
        switch!(app, "faster-hacking", "Faster Hacking Exit", "A successful hack opens the terminal after half a second. (Stewie's Tweaks bFasterHackingTransition)", game.faster_hacking_transition);
        // ---- conversations ----
        part!(app, "dialogue-music", "Music While Talking", "The music's volume in a conversation, as a part of its own; 100% changes nothing. (Stewie's Tweaks bScaleMusicVolumeDuringDialogue)", dialogue_music_volume);
        part!(app, "dialogue-radio", "Radio While Talking", "The radio's volume in a conversation, as a part of its own; 100% changes nothing. (Stewie's Tweaks bScaleRadioSongVolumeDuringDialogue)", dialogue_radio_volume);
        // ---- batch 4 ----
        switch!(app, "console-skips-player", "Console Clicks Skip the Player", "Clicking in the console never selects the player. (Stewie's Tweaks bNonSelectablePlayerInConsole)", console_skips_player);
        switch!(app, "holster-real-time", "Holster in Real Time", "Holding Ready Item to holster counts real time, so slowed time does not lengthen it. (Stewie's Tweaks bPipboyLightAndHolsteringIgnoreTimescale)", holster_real_time);
        switch!(app, "power-armor-no-screen-blood", "No Screen Blood in Power Armor", "Hits put no blood on the screen while power armor is worn. (Stewie's Tweaks bPowerArmorPreventsScreenBlood)", power_armor_no_screen_blood);
        switch!(app, "no-red-crosshair-cloaked", "Invisible Foes Stay Hidden", "An invisible foe does not turn the crosshair red. (Stewie's Tweaks bHideRedCrosshairOnDistantInvisibleTargets)", no_red_crosshair_on_cloaked);
        switch!(app, "god-mode-quiet-landing", "Quiet Landings in God Mode", "In god mode the player's hard landings make no pain sound. (Stewie's Tweaks bGodmodePreventsLegCrippleSound)", god_mode_quiet_landing);
        switch!(app, "console-compress-repeats", "Count Repeated Console Lines", "A console line said again right after itself is counted on one line (line -#2). (Stewie's Tweaks bCompressDuplicateConsoleMessages)", console_compress_repeats);
        switch!(app, "console-timestamps", "Console Times", "Console lines start with the time since the game started. (Stewie's Tweaks bConsolePrintsIncludeTimestamp)", console_timestamps);
        switch!(app, "living-anatomy-dr", "Living Anatomy Shows DR", "With Living Anatomy the target's line shows its Damage Resistance as well. (Stewie's Tweaks bLivingAnatomyShowDR)", living_anatomy_dr);
        switch!(app, "sync-categories", "Matching Categories", "In containers and barter both lists change category together. (Stewie's Tweaks bSynchronizeContainerCategories)", game.sync_categories);
        switch!(app, "sync-categories-keep-empty", "Matching Categories: Keep Empty", "Stay on an emptied synchronized category after moving its last item instead of switching to All. (Stewie's Tweaks bPreventSwitchWhenEmptyingCategory)", game.prevent_switch_when_emptying_category);
        // ---- batch 6: gameplay ----
        add_choice_list(
            app,
            Setting {
                key: "tweak-perks-per-level",
                group: GROUP,
                label: "Perks per Level",
                note: "How many perks a level-up's perk page lets the player take; 1 changes nothing. (Stewie's Tweaks iPerksPerLevel)",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            ChoiceList {
                names: |_| choice_names(&PERK_CHOICES),
                chosen: |w| PERK_CHOICES[w.get_resource::<TweakSettings>().map_or(1, |t| t.game.perks_per_level).clamp(1, PERKS) - 1].to_string(),
                pick: take_perks_per_level,
                save: |w| PERK_CHOICES[w.get_resource::<TweakSettings>().map_or(1, |t| t.game.perks_per_level).clamp(1, PERKS) - 1].to_string(),
                load: take_perks_per_level,
                ask_keep: false,
            },
        );
        add_setting(
            app,
            Setting::slider(
                "tweak-special-points",
                GROUP,
                "S.P.E.C.I.A.L. Points",
                "The total the S.P.E.C.I.A.L. menu lets the player share out (when no script set one); 40 changes nothing. (Stewie's Tweaks bCustomSpecialPoints)",
                Slider::choice(
                    SPECIALS,
                    |w| choice_part(special_index(w.get_resource::<TweakSettings>().map_or(fnv_game::chargen::SPECIAL_POINTS, |t| t.game.special_points)), SPECIALS),
                    |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.special_points = special_total(choice_index(v, SPECIALS));
                        }
                    },
                    |v| special_total(choice_index(v, SPECIALS)).to_string(),
                ),
            ),
        );
        switch!(app, "pick-without-skill", "Pick Any Lock", "A lock above the player's Lockpick skill can still be picked. (Stewie's Tweaks bPickLocksWithoutSkill)", game.pick_locks_without_skill);
        switch!(app, "hack-without-skill", "Hack Any Terminal", "A terminal above the player's Science skill can still be hacked. (Stewie's Tweaks bHackTerminalsWithoutSkill)", game.hack_terminals_without_skill);
        switch!(app, "no-terminal-lockout", "No Terminal Lockout", "A failed hack does not lock the terminal. (Stewie's Tweaks bNoLockFailedTerminals)", game.no_terminal_lockout);
        switch!(app, "no-casino-bans", "No Casino Bans", "Winning big never bans the player from a casino. (Stewie's Tweaks bNoCasinoBans)", game.no_casino_bans);
        switch!(app, "loot-unconscious", "Search the Unconscious", "An unconscious actor can be searched like a dead one. (Stewie's Tweaks bLootUnconsciousVictims)", game.loot_unconscious);
        switch!(app, "revive-companions", "Revive Unconscious Companions", "Activating a knocked-out companion uses a Stimpak and gets them up at once. (Stewie's Tweaks bReviveUnconsciousCompanions)", game.revive_companions);
        amount!(app, "revive-down-seconds", "Revive: Companions Stay Down", "Least seconds a revivable companion stays down (and in a fight); 0 keeps 10. (Stewie's Tweaks bReviveUnconsciousCompanions, fUnconsciousTime)", 600.0, f32, game.revive_down_seconds);
        switch!(app, "revive-on-cell-change", "Revive: On Changing Cells", "Knocked-out companions get up automatically on a worldspace or interior change. (Stewie's Tweaks bReviveUnconsciousCompanions, bReviveWhenChangingCells)", game.revive_on_cell_change);
        switch!(app, "pickpocket-worn", "Pickpocket Worn Items", "What an actor wears can be pickpocketed (never a child's). (Stewie's Tweaks bPickpocketWornItems)", game.pickpocket_worn_items);
        switch!(app, "no-sneak-criticals", "No Sneak Criticals", "The player's sneak attacks are not criticals. (Stewie's Tweaks bNoSneakAttacksCriticals)", game.no_sneak_criticals);
        switch!(app, "reload-when-empty", "Reload Only When Empty", "The player cannot reload a magazine that still has rounds. (Stewie's Tweaks bDisableReloadingNonEmptyClip)", game.reload_only_when_empty);
        switch!(app, "no-travel-time", "Fast Travel Takes No Time", "Fast travel moves the clock (and hardcore needs) on by nothing. (Stewie's Tweaks bNoFastTravelTimeChange)", no_travel_time);
        add_setting(
            app,
            Setting::slider(
                "tweak-power-armor-fall",
                GROUP,
                "Falls in Power Armor",
                "Fall damage to anyone in power armor is multiplied by this; x1.0 changes nothing. (Stewie's Tweaks bPowerArmorScalesFallDamage)",
                Slider {
                    get: |w| part(w.get_resource::<TweakSettings>().map_or(1.0, |t| f64::from(t.power_armor_fall_damage))),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.power_armor_fall_damage = factor(v) as f32;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(shown_factor),
                },
            ),
        );
        // ---- weight ----
        weightless!(app, "weightless-armor", "Weightless Armor", "Armor weighs nothing. (Stewie's Tweaks bWeightlessItems, bArmor)", fnv_sim::inventory::WEIGHTLESS_ARMOR);
        weightless!(app, "weightless-aid", "Weightless Aid", "Aid items weigh nothing. (Stewie's Tweaks bWeightlessItems, bAid)", fnv_sim::inventory::WEIGHTLESS_AID);
        weightless!(app, "weightless-ammo", "Weightless Ammo", "Ammo weighs nothing (it only weighs anything in Hardcore). (Stewie's Tweaks bWeightlessItems, bAmmo)", fnv_sim::inventory::WEIGHTLESS_AMMO);
        weightless!(app, "weightless-weapons", "Weightless Weapons", "Weapons weigh nothing. (Stewie's Tweaks bWeightlessItems, bWeapons)", fnv_sim::inventory::WEIGHTLESS_WEAPONS);
        weightless!(app, "weightless-misc", "Weightless Misc Items", "Misc items weigh nothing. (Stewie's Tweaks bWeightlessItems, bMisc)", fnv_sim::inventory::WEIGHTLESS_MISC);
        switch!(app, "weightless-worn-armor", "Worn Armor Weighs Nothing", "Armor weighs nothing while worn. (Stewie's Tweaks bWeightlessWornArmor)", game.weightless_worn_armor);
        switch!(app, "show-poison-effects", "Show Weapon Poison Effects", "The Pip-Boy's weapon card names the poison on the weapon, its hits left and its effects. (Stewie's Tweaks bShowWeaponPoisonEffects)", game.show_poison_effects);
        switch!(app, "no-poison-confirm", "No Poison Confirmation", "A poison goes on the weapon at the first use, without asking for a second. (Stewie's Tweaks bNoPoisonConfirm)", game.no_poison_confirm);
        switch!(app, "map-summon-companions", "Summon Companions from the Map", "Clicking a companion's name on the World Map brings them to the player's side (every companion under the click). (Stewie's Tweaks bMapSelectableCompanions)", game.map_summon_companions);
        switch!(app, "summon-unconscious", "Summon Companions: Knocked-Out Too", "Knocked-out companions shown on the map can be summoned too. (Stewie's Tweaks bMapSelectableCompanions, bAllowSummonUnconsious)", game.summon_unconscious);
        switch!(app, "no-min-companion-distance", "Companions Near You on the Map", "The World Map names companions even when they are close to the player. (Stewie's Tweaks bNoMinCompanionMapDistance)", game.no_min_companion_distance);
        switch!(app, "map-unconscious-companions", "Knocked-Out Companions on the Map", "The World Map names knocked-out companions too, in red. (Stewie's Tweaks bMapShowUnconsciousCompanions)", game.map_unconscious_companions);
        switch!(app, "companions-location-font", "Companions in the Locations' Font", "The World Map writes companions' names in the font it names locations in. (Stewie's Tweaks bCompanionsUseLocationFontOnMap)", game.companions_location_font);
        switch!(app, "map-door-kinds", "Exterior and Interior Door Icons", "Local map doors out look unlike doors in (Stewie's Tweaks' pictures if installed, else dimmer). (Stewie's Tweaks bInteriorExteriorMapDoorIcon)", game.map_door_kinds);
        switch!(app, "hostiles-prefer-player", "Hostiles Prioritize Player", "An enemy about to fight one of the player's companions may fight the player instead. (Stewie's Tweaks bHostilesPrioritizePlayer)", game.hostiles_prefer_player);
        part!(app, "prefer-player-start", "Hostiles Prioritize Player: At Start", "The chance an enemy turns to the player as its fight begins. (Stewie's Tweaks bHostilesPrioritizePlayer, fSwapToPlayerChanceStartCombat)", game.prefer_player_start);
        part!(app, "prefer-player-mid", "Hostiles Prioritize Player: Mid-Fight", "The chance an enemy turns to the player when it picks a companion later in the fight. (Stewie's Tweaks bHostilesPrioritizePlayer, fSwapToPlayerChanceMidCombat)", game.prefer_player_mid);
        amount!(app, "prefer-player-detection", "Hostiles Prioritize Player: Detection", "How well the enemy must detect the player to turn to them. (Stewie's Tweaks bHostilesPrioritizePlayer, iPlayerDetectionThreshold)", 100.0, f32, game.prefer_player_detection);
        switch!(app, "equip-broken-items", "Equip Broken Items", "A broken weapon can be equipped, to repair it with a repair kit; it does not fire or swing until repaired. (Stewie's Tweaks bEquipBrokenItems)", game.equip_broken_items);
        switch!(app, "keep-broken-equipped", "Keep Broken Items Equipped", "A weapon that breaks stays in the hand, with no message; it does not fire or swing until repaired. (Stewie's Tweaks bKeepBrokenItemsEquipped)", game.keep_broken_equipped);
        switch!(app, "hardcore-tweaks", "Hardcore Rules One by One", "Three of Hardcore's rules are set below on their own, whatever the Hardcore setting. (Stewie's Tweaks bHardcoreTweaks)", game.hardcore_tweaks);
        switch!(app, "hardcore-ammo-weight", "Hardcore Rules: Ammo Has Weight", "Ammunition counts toward the weight carried. (Stewie's Tweaks bHardcoreTweaks, bAmmoWeight)", game.hardcore_ammo_weight);
        switch!(app, "hardcore-essential-companions", "Hardcore Rules: Companions Cannot Die", "Companions are knocked out instead of killed. (Stewie's Tweaks bHardcoreTweaks, bEssentialCompanions)", game.hardcore_essential_companions);
        switch!(app, "hardcore-sleep-heals", "Hardcore Rules: Sleep Heals", "Sleeping restores Health and heals crippled limbs (a bed's own script may heal anyway). (Stewie's Tweaks bHardcoreTweaks, bSleepingHeals)", game.hardcore_sleep_heals);
        switch!(app, "weightless-worn-power-armor", "Worn Power Armor Weighs Nothing", "Power armor weighs nothing while worn. (Stewie's Tweaks bWeightlessWornPowerArmor)", game.weightless_worn_power_armor);
        // ---- batch 8 ----
        switch!(app, "no-travel-crippled-leg", "No Travel on a Crippled Leg", "Fast travel is refused while a leg is crippled. (Stewie's Tweaks bNoFastTravelIfLegsCrippled)", no_travel_crippled_leg);
        switch!(app, "wait-only-seated", "Wait Only When Seated", "Waiting is refused unless the player sits (god mode excepted). (Stewie's Tweaks bOnlyAllowWaitWhileSitting)", game.wait_only_seated);
        amount!(app, "kill-reward-ap", "Action Points per Kill", "Action Points a kill by the player gives back; 0 changes nothing. (Stewie's Tweaks bKillsRewardAP)", 50.0, f64, game.kill_reward_ap);
        amount!(app, "jump-ap-cost", "Action Points per Jump", "Action Points a jump costs; with too few the player cannot jump. 0 changes nothing. (Stewie's Tweaks bJumpingCostsAP)", 50.0, f32, jump_ap_cost);
        switch!(app, "jump-ap-danger-only", "Action Points per Jump: Only in Danger", "With Action Points per Jump above 0, jumps only cost AP while in danger. (Stewie's Tweaks bCombatOnly)", jump_ap_danger_only);
        switch!(app, "jump-without-enough-ap", "Action Points per Jump: Allow Low AP", "With Action Points per Jump above 0, still jump with too little AP, draining what is left. (Stewie's Tweaks bJumpWithoutEnoughAP)", jump_without_enough_ap);
        amount!(app, "run-ap-cost", "Action Points per Second Running", "Action Points running costs each second; with none left the player walks. 0 changes nothing. (Stewie's Tweaks bRunningCostsAP)", 20.0, f32, run_ap_per_second);
        amount!(app, "sneak-run-ap-cost", "Action Points per Second Sneak-Running", "Sneak-running costs this many AP a second; with none left the player walks. 0 changes nothing. (Stewie's Tweaks bRunningCostsAP, fSneakAPDrainCostBase)", 20.0, f32, sneak_run_ap_base);
        add_setting(
            app,
            Setting::slider(
                "tweak-sneak-run-ap-endurance",
                GROUP,
                "Sneak-Running AP: Endurance Factor",
                "Added per Endurance over 5 to Sneak-Running's AP cost (negative saves AP); 0 changes nothing. (Stewie's Tweaks fSneakRunAPEnduranceMult)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.5, |t| ((t.sneak_run_ap_endurance_mult + 1.0) / 2.0).clamp(0.0, 1.0)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.sneak_run_ap_endurance_mult = ((v * 2.0 - 1.0) * 10.0).round() / 10.0;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| {
                        let rate = ((v * 2.0 - 1.0) * 10.0).round() / 10.0;
                        if rate == 0.0 { "0.0".to_string() } else { format!("{rate:+.1}") }
                    }),
                },
            ),
        );
        // ---- batch 9 ----
        switch!(app, "no-selling", "No Selling", "The player cannot sell items in a barter (buying still works). (Stewie's Tweaks bDisableSellingItems)", no_selling);
        add_setting(
            app,
            Setting {
                key: "tweak-custom-skill-points",
                group: GROUP,
                label: "Own Skill Points Rule",
                note: "A level-up gives the base below plus Intelligence times the amount below, rounded up. (Stewie's Tweaks bModifySkillPointsEarned)",
                switches: "",
                get: |w| w.get_resource::<TweakSettings>().is_some_and(|t| t.game.skill_points.is_some()),
                set: |w, on| {
                    if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                        let numbers = (t.skill_point_base, t.skill_points_per_int);
                        t.game.skill_points = on.then_some(numbers);
                    }
                },
                slider: None,
            },
        );
        add_setting(
            app,
            Setting::slider(
                "tweak-skill-point-base",
                GROUP,
                "Skill Points: Base",
                "The base of the Own Skill Points Rule (0 to 30). (Stewie's Tweaks iSkillPointBase)",
                Slider::choice(
                    SKILL_BASES,
                    |w| choice_part(w.get_resource::<TweakSettings>().map_or(SKILL_POINTS_START.0, |t| t.skill_point_base) as usize, SKILL_BASES),
                    |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.skill_point_base = choice_index(v, SKILL_BASES) as i32;
                            t.apply_skill_points();
                        }
                    },
                    |v| choice_index(v, SKILL_BASES).to_string(),
                ),
            ),
        );
        add_setting(
            app,
            Setting::slider(
                "tweak-skill-points-per-int",
                GROUP,
                "Skill Points: per Intelligence",
                "Points per point of Intelligence in the Own Skill Points Rule (0 to 2). (Stewie's Tweaks fPointsPerInt)",
                Slider {
                    get: |w| (w.get_resource::<TweakSettings>().map_or(SKILL_POINTS_START.1, |t| t.skill_points_per_int) / 2.0) as f32,
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.skill_points_per_int = f64::from(v) * 2.0;
                            t.apply_skill_points();
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.1}", v * 2.0)),
                },
            ),
        );
        switch!(app, "no-unconscious-message", "No Unconscious Message", "No line saying an actor is unconscious. (Stewie's Tweaks bNoUnconciousMessage)", game.no_unconscious_message);
        switch!(app, "no-voice-filter", "No Helmet Voice Filter", "Voices from behind helmets and speakers play unfiltered. (Stewie's Tweaks bNoAudioDistortion)", game.no_voice_filter);
        switch!(app, "no-skip-dialogue", "No Skipping Lines", "A click or key does not cut a spoken line short. (Stewie's Tweaks bNoSkipDialog)", game.no_skip_dialogue);
        switch!(app, "lockpick-mouse", "Mouse Turns the Lock", "In lockpicking, the left mouse button turns the screwdriver as the movement keys do. (Stewie's Tweaks bLockpickAllowMouse)", game.lockpick_mouse_turn);
        switch!(app, "no-max-casino-bet", "No Casino Bet Limit", "Blackjack, roulette and the slots take any bet the player can pay. (Stewie's Tweaks bNoMaxCasinoBet)", game.no_max_casino_bet);
        switch!(app, "no-aiming-slowdown", "Full Pace While Aiming", "Aiming or blocking does not slow the player to a walk. (Stewie's Tweaks bAimingSpeed)", no_aiming_slowdown);
        switch!(app, "repeat-jumping", "Hold to Keep Jumping", "Holding Jump jumps again on landing. (Stewie's Tweaks bRepeatJumping)", repeat_jumping);
        switch!(app, "hold-to-sneak", "Hold to Sneak", "Sneak only while the Crouch/Sneak key is held. (Stewie's Tweaks bHoldCrouchToSneak)", hold_to_sneak);
        // ---- batch 10 ----
        amount!(app, "vats-entry-ap", "Action Points to Enter V.A.T.S.", "Action Points entering V.A.T.S. costs; refused without them. 0 changes nothing. (Stewie's Tweaks bEnteringVATSCostsAP)", 50.0, f64, game.vats_entry_ap);
        amount!(app, "power-attack-ap", "Action Points per Power Attack", "Action Points a power attack costs; 0 changes nothing. (Stewie's Tweaks bChargedAttacksCostAP)", 50.0, f32, power_attack_ap);
        amount!(app, "quest-marker-reach", "Quest Markers Within", "Quest targets farther than this (game units) are left off the compass; 0 changes nothing. (Stewie's Tweaks bDistanceBasedQuestMarkerVisibilty)", 20000.0, f32, quest_marker_reach);
        part!(app, "scope-zoom-time", "Scope Zoom Time", "How long a scope takes to zoom, as a part of the usual; 100% changes nothing. (Stewie's Tweaks bUseCustomSniperZoomRate)", scope_zoom_time);
        switch!(app, "drop-key", "Q Drops Items", "In the Pip-Boy's inventory, Q drops the selected item. (Stewie's Tweaks bAddInventoryDropItemHotkey)", drop_key);
        // ---- batch 11 ----
        switch!(app, "no-container-after-pick", "Picked Containers Stay Shut", "A container stays shut after its lock is picked; open it yourself. (Stewie's Tweaks bDontShowContainerAfterLockpick)", game.no_container_after_pick);
        switch!(app, "no-door-after-pick", "Picked Doors Stay Shut", "A door is not used at once after its lock is picked. (Stewie's Tweaks bDontOpenDoorsAfterLockpick)", game.no_door_after_pick);
        switch!(app, "mark-unvisited-doors", "Mark Unvisited Doors", "A door to a place never visited ends its prompt with a star. (Stewie's Tweaks bPatchUnseenCellName)", game.mark_unvisited_doors);
        switch!(app, "no-exit-hacking", "No Leaving a Hack", "Once a guess is made, the hacking game cannot be left before it ends. (Stewie's Tweaks bNoExitHacking)", game.no_exit_hacking);
        switch!(app, "no-grab-owned", "No Grabbing Owned Items", "Someone else's things cannot be picked up with Grab. (Stewie's Tweaks bNoGrabOwnedItems)", game.no_grab_owned);
        // ---- batch 12 ----
        add_setting(
            app,
            Setting::slider(
                "tweak-agility-speed",
                GROUP,
                "Agility Moves Faster",
                "Everyone's speed gains this much per point of Agility above 5, and loses it below; 0% changes nothing. (Stewie's Tweaks bAgilityScalesMovementSpeed)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| (t.game.agility_speed_per_point * 10.0) as f32),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.agility_speed_per_point = f64::from(v) / 10.0;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.1}%", v * 10.0)),
                },
            ),
        );
        switch!(app, "auto-unlock-terminals", "Skip Easy Hacks", "A terminal opens without the hacking game when Science is at least its lock plus the margin below. (Stewie's Tweaks bAutoUnlockTerminals)", game.auto_unlock_terminals);
        amount!(app, "auto-unlock-margin", "Skip Easy Hacks: Margin", "How far over the lock Science must be to skip the hacking game (0 to 50). (Stewie's Tweaks bAutoUnlockTerminals, iThreshold)", 50.0, f32, game.auto_unlock_margin);
        switch!(app, "weapons-unhurt-by-hits", "Shots Spare Weapons", "A shot that hits a weapon in hand does not wear it. (Stewie's Tweaks bHittingWeaponsDoesntDamageThem)", game.weapons_unhurt_by_hits);
        switch!(app, "god-mode-no-jams", "No Jams in God Mode", "In god mode the player's weapon never jams. (Stewie's Tweaks bGodModePreventsJamAnims)", game.god_mode_no_jams);
        switch!(app, "knockdown-by-strength", "Strength Resists Blasts", "An explosion's knockdown is resisted by Strength instead of Agility. (Stewie's Tweaks bExplosionKnockdownAvoidanceUsesStrength)", game.knockdown_by_strength);
        switch!(app, "drowning-drains-ap", "Drowning Takes AP First", "Drowning takes Action Points before health. (Stewie's Tweaks bDrowningDrainsAP)", game.drowning_drains_ap);
        switch!(app, "aiming-draws-weapon", "Aiming Draws the Weapon", "Aiming with the weapon holstered draws it. (Stewie's Tweaks bAimingUnholstersWeapons)", aiming_draws_weapon);
        add_setting(
            app,
            Setting::slider(
                "tweak-walk-angle",
                GROUP,
                "Steepest Walkable Slope",
                "The steepest slope the player can walk up, in degrees; 45 is the usual. (Stewie's Tweaks bBillyGoatMode)",
                Slider::choice(
                    ANGLES,
                    |w| choice_part(((w.get_resource::<TweakSettings>().map_or(45.0, |t| t.walk_angle) - 30.0) / 5.0).round().clamp(0.0, (ANGLES - 1) as f32) as usize, ANGLES),
                    |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.walk_angle = walk_angle(choice_index(v, ANGLES));
                        }
                    },
                    |v| format!("{:.0} degrees", walk_angle(choice_index(v, ANGLES))),
                ),
            ),
        );
        part!(app, "crippled-leg-jump", "Jumping on a Crippled Leg", "The jump's height for each crippled leg, as a part of the usual; 100% changes nothing. (Stewie's Tweaks bCrippledLegsScaleJumpHeight)", crippled_leg_jump);
        // ---- batch 13 ----
        switch!(app, "aid-at-full-health", "Aid at Full Health", "The STATS page's Stimpak and Doctor's Bag can be used with nothing hurt. (Stewie's Tweaks bAllowAidAtMaxHealth)", game.aid_at_full_health);
        switch!(app, "grabbing-is-theft", "Grabbing Is Stealing", "Grabbing someone else's thing counts as stealing it. (Stewie's Tweaks bGrabbingItemsIsCrime)", game.grabbing_is_theft);
        amount!(app, "grabbing-is-theft-min-value", "Grabbing Is Stealing: Min Value", "Lowest item value (caps) that makes grabbing an owned item a crime; 0 counts any item. (Stewie's Tweaks bGrabbingItemsIsCrime, fMinValue)", 500.0, f32, game.grabbing_is_theft_min_value);
        switch!(app, "vendor-lists", "Merchants Keep to Their Lists", "A merchant buys from the player only what its list says it deals in. (Stewie's Tweaks bBarterCheckActorBuySellFlags)", game.vendor_lists_both);
        switch!(app, "key-name", "Name the Key", "A lock that needs a key names it. (Stewie's Tweaks bLockNeedsKeyShowName)", game.lock_needs_key_name);
        switch!(app, "no-skill-tags", "No Skill Tags", "Dialogue topics show no skill or perk tag. (Stewie's Tweaks bNoSkillTags)", game.no_skill_tags);
        switch!(app, "no-npc-addiction", "No NPC Addiction", "Only the player becomes addicted to chems. (Stewie's Tweaks bPreventNPCAddiction)", game.no_npc_addiction);
        switch!(app, "pickpocket-fleeing", "Pickpocket the Fleeing", "An actor running away can be pickpocketed. (Stewie's Tweaks bPickpocketFleeingActors)", game.pickpocket_fleeing);
        switch!(app, "full-reload-swaps", "Full Reload Swaps Ammo", "Reloading a full magazine changes to the next ammo. (Stewie's Tweaks bReloadingWithFullClipSwitchesAmmoType)", game.full_reload_swaps_ammo);
        add_setting(
            app,
            Setting::slider(
                "tweak-melee-reach",
                GROUP,
                "Melee Reach",
                "The player's melee reach is multiplied by this; x1.0 changes nothing. (Stewie's Tweaks bScalePlayerMeleeReach)",
                Slider {
                    get: |w| part(w.get_resource::<TweakSettings>().map_or(1.0, |t| f64::from(t.game.melee_reach_mult))),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.melee_reach_mult = factor(v) as f32;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(shown_factor),
                },
            ),
        );
        amount!(app, "repair-xp", "Experience per Repair", "Experience a Pip-Boy repair gives; 0 changes nothing. (Stewie's Tweaks bRepairsRewardXP)", 50.0, f64, game.repair_xp);
        switch!(app, "repair-caps-container", "Repair Caps Go to the Till", "Caps paid for a repair go into the merchant's container, not its pockets. (Stewie's Tweaks bPreventStealingCapsAfterRepair)", game.repair_caps_to_container);
        switch!(app, "no-wear-on-dead", "Spare the Blade on the Dead", "Melee blows that land only on the dead do not wear the weapon. (Stewie's Tweaks bNoDamageMeleeWeaponIfTargetDead)", game.no_wear_on_dead);
        add_setting(
            app,
            Setting::slider(
                "tweak-encumbered-ap-regen",
                GROUP,
                "Action Points Back When Over-Encumbered",
                "How fast Action Points come back while over-encumbered, as a part of the usual; 100% changes nothing. (Stewie's Tweaks bNoAPRegenWhileOverencumbered)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(1.0, |t| t.game.encumbered_ap_regen as f32),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.encumbered_ap_regen = f64::from(v);
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: None,
                },
            ),
        );
        switch!(app, "light-step-companions", "Light Step for Companions", "Teammates are spared mines whenever the player would be (Light Step). (Stewie's Tweaks bLightStepAffectsCompanions)", game.light_step_companions);
        switch!(app, "no-player-stagger", "No Player Stagger", "The player never staggers from a hit. (Stewie's Tweaks bNoPlayerStaggerAnims)", no_player_stagger);
        switch!(app, "ready-does-not-draw", "Ready Does Not Draw", "Ready Item reloads or holsters a drawn weapon but does not draw a holstered one. (Stewie's Tweaks bPreventReloadWeaponUnholstering)", ready_does_not_draw);
        // ---- batch 14 ----
        switch!(app, "jam-reload-pace", "Jams at Reload Pace", "Clearing a jam takes as long as the reload's own pace makes it. (Stewie's Tweaks bReloadJamsAffectedByAgility)", game.jam_at_reload_pace);
        switch!(app, "no-hotkeys-reloading", "No Hotkeys While Reloading", "Weapon hotkeys do nothing while reloading. (Stewie's Tweaks bReloadingPreventsWeaponHotkeys)", no_hotkeys_while_reloading);
        part!(app, "wading-pace", "Wading Pace", "The pace in water up to swimming depth, as a part of the usual (shallower water slows less); 100% changes nothing. (Stewie's Tweaks bRunSlowerInWater)", wading_pace);
        switch!(app, "sleep-on-chairs", "Sleep on Chairs", "Waiting while sitting on a chair is sleeping. (Stewie's Tweaks bSleepOnChairs)", game.sleep_on_chairs);
        switch!(app, "stealing-no-alarm", "Quiet Theft", "Taking someone's things raises no alarm; pickpocketing still does. (Stewie's Tweaks bStealingSendsNoAlarm)", game.stealing_no_alarm);
        switch!(app, "no-companion-kill-xp", "No XP for Companion Kills", "A teammate's kills give the player no experience. (Stewie's Tweaks bNoCompanionKillXP)", game.no_companion_kill_xp);
        switch!(app, "no-disarm-companions", "Companions Keep Their Weapons", "A crippled arm does not make a teammate drop its weapon. (Stewie's Tweaks bNoDisarmCompanions)", game.no_disarm_companions);
        switch!(app, "no-self-explosion-damage", "Own Blasts Spare the Player", "The player's own explosions do not hurt the player. (Stewie's Tweaks bNoSelfExplosionDamage)", game.no_self_explosion_damage);
        switch!(app, "no-self-explosion-non-hostiles", "Own Blasts Spare Non-Hostiles", "With Own Blasts Spare the Player on, your explosions also spare friendly and neutral actors. (Stewie's Tweaks bNoSelfExplosionDamage)", game.no_self_explosion_non_hostiles);
        switch!(app, "sneak-criticals-melee", "Sneak Criticals Melee Only", "Only melee and unarmed sneak attacks are criticals. (Stewie's Tweaks bSneakCriticalsOnlyMeleeWeapons)", game.sneak_criticals_melee_only);
        switch!(app, "teammates-meltdown", "Meltdown for Companions", "Teammates' energy weapon kills use the player's Meltdown perk. (Stewie's Tweaks bAllowTeammatesUseMeltdown)", game.teammates_meltdown);
        switch!(app, "speed-scales-jump", "Speed Scales the Jump", "The player's Speed Mult (and armour's pace) scales the jump's height. (Stewie's Tweaks bSpeedMultScalesJumpHeight)", game.speed_scales_jump);
        // ---- batch 15 ----
        switch!(app, "no-tutorials", "No Minigame Help", "Lockpicking, blackjack, roulette and the slots show no first-time help. (Stewie's Tweaks bDisableTutorialMessages)", game.no_tutorials);
        switch!(app, "no-knockdown-god", "No Knockdowns in God Mode", "In god mode the player is never knocked down. (Stewie's Tweaks bNoKnockdownInGodmode)", game.no_knockdown_in_god_mode);
        switch!(app, "show-current-skill", "Say My Skill", "When a lock or terminal needs more skill, the line says the player's own. (Stewie's Tweaks bLockpickHackingMessageShowsCurrentSkill)", game.show_current_skill);
        amount!(app, "min-splatter", "Blood Only on Big Hits", "Hits that take less health than this put no blood on the screen; 0 changes nothing. (Stewie's Tweaks bMinBloodSplatterHealthDamage)", 50.0, f32, min_splatter_damage);
        switch!(app, "xp-sound-at-cap", "XP Sound at the Level Cap", "Experience earned at the level cap still plays its sound. (Stewie's Tweaks bMinigamesPlayXPSoundAtMaxLevel)", xp_sound_at_cap);
        switch!(app, "hold-to-wait", "Hold to Wait", "The Wait key opens the menu when held half a second, not when tapped. (Stewie's Tweaks bHoldWaitKeyToShowMenu)", hold_to_wait);
        switch!(app, "no-sight-zoom", "No Iron Sight Zoom", "Aiming down iron sights does not zoom the view (scopes still do). (Stewie's Tweaks bDisableWeaponFOV)", no_sight_zoom);
        switch!(app, "hide-misc-quest", "Hide Quest Items in Misc", "The Pip-Boy's Misc list leaves quest items out. (Stewie's Tweaks bHideMiscQuestItems)", hide_misc_quest);
        switch!(app, "tab-click-sorts", "Clicking the Tab Sorts", "In the Pip-Boy's items, clicking the open tab sorts by name, value, then weight, most first. (Stewie's Tweaks bClickingActiveTabTogglesSorting)", tab_click_sorts);
        // ---- batch 16 ----
        switch!(app, "hotkey-toggles-in-hand", "Hotkey Draws and Holsters", "Pressing the hotkey of the weapon in hand draws or holsters it. (Stewie's Tweaks bHotkeyHolstersWeaponIfEquipped)", hotkey_toggles_in_hand);
        switch!(app, "no-interior-fog", "No Interior Fog", "Interiors have no fog (from the next room entered). (Stewie's Tweaks bDisableInteriorFog)", no_interior_fog);
        switch!(app, "mark-unspoken", "Mark Strangers", "Someone not yet spoken to ends their prompt with a star. (Stewie's Tweaks bUnspokenNPCIndicator)", game.mark_unspoken);
        switch!(app, "no-caps-added", "No Caps Added Line", "Getting caps shows no line. (Stewie's Tweaks bHideCapsAddedMessages)", game.no_caps_added_message);
        switch!(app, "subtitle-names", "Subtitles Name the Speaker", "General subtitles start with the speaker's name. (Stewie's Tweaks bSubtitlesShowActorNames)", game.subtitle_names);
        switch!(app, "compass-pips-danger", "Compass Pips Only in Danger", "Actors show on the compass only while in danger. (Stewie's Tweaks bNoCompassPipsIfNotInDanger)", compass_pips_in_danger_only);
        switch!(app, "neutral-foes-compass", "Foes in the Usual Colour", "Hostile actors show on the compass in the HUD colour, not red. (Stewie's Tweaks bHostilesUseNeutralColorOnCompass)", neutral_foes_on_compass);
        switch!(app, "sneak-label-standing", "Sneak Label While Standing", "The Hidden / Detected label shows while standing too. (Stewie's Tweaks bShowSneakLabelWhileStanding)", sneak_label_standing);
        switch!(app, "no-xp-meter-combat", "No XP Meter in Combat", "Experience gained in a fight shows once it is over, all together. (Stewie's Tweaks bNoXPBarInCombat)", no_xp_meter_in_combat);
        switch!(app, "no-limb-indicator", "No Crippled Limb Figure", "The crippled-limb figure stays off the HUD. (Stewie's Tweaks bDisableHUDCrippledLimbIndicator)", no_limb_indicator);
        switch!(app, "skip-death-cam", "Skip Death Camera", "After you die, Left Alt goes straight to the end of the death camera: the screen black, ready to load. (Stewie's Tweaks bSkipDeathcamHotkey)", skip_death_cam);
        amount!(app, "skip-death-cam-attack", "Skip Death Camera: By Attacking After", "Skip Death Camera: attacking skips it too, this long after death; 0: never. (Stewie's Tweaks bSkipDeathcamHotkey, fAttackDebounceTime)", 10.0, f32, skip_death_cam_attack);
        switch!(app, "help-is-console", "Console in the Pause Menu", "The pause menu has a Console entry where the original has Help: the game goes on with the console open. (Stewie's Tweaks bReplaceHelpWithConsole)", help_is_console);
        switch!(app, "heartbeat-fade", "Heartbeat Fades", "The low-health heartbeat fades away, and comes back at full strength each time you are hurt. (Stewie's Tweaks bHeatbeatSoundsFade)", heartbeat_fade);
        amount!(app, "heartbeat-fade-seconds", "Heartbeat Fades: Seconds", "How long the heartbeat takes to fade away. (Stewie's Tweaks bHeatbeatSoundsFade, fDuration)", 30.0, f32, heartbeat_fade_seconds);
        switch!(app, "terminal-fade", "Terminal Fade Time", "A terminal's screen fades out over the time below when it closes, instead of a quarter second. (Stewie's Tweaks bTerminalFadeTime)", terminal_fade);
        amount!(app, "terminal-fade-seconds", "Terminal Fade Time: Seconds", "How long a closing terminal takes to fade out; 0 closes it at once. (Stewie's Tweaks bTerminalFadeTime, fFadeLength)", 5.0, f32, terminal_fade_seconds);
        switch!(app, "containers-show-ammo", "Containers Show Equipped Ammo", "In a container, the ammo your weapon loads has the equipped square, as in the Pip-Boy. (Stewie's Tweaks bContainersShowEquippedAmmo)", containers_show_ammo);
        switch!(app, "descriptive-marker-added", "Map Marker Added Names the Place", "The message when a place is put on your map names it. (Stewie's Tweaks bDescriptiveMarkerAddedMessage)", descriptive_marker_added);
        switch!(app, "no-grenade-indicator", "Hide Grenade Indicator", "No mark on the HUD for a grenade about to go off nearby. (Stewie's Tweaks bDisableGrenadeIndicator)", no_grenade_indicator);
        switch!(app, "grenade-indicator-harmless", "No Indicator for Harmless Blasts", "Grenades whose explosion does no damage get no mark. (Stewie's Tweaks bHideGrenadeIndicatorForNoDamageExplosions)", grenade_indicator_skip_harmless);
        switch!(app, "grenade-indicator-destructibles", "Mark Objects About to Explode", "Burning objects that will explode, such as cars on fire, get a grenade mark too. (Stewie's Tweaks bExplodingDestructibleIndicator)", grenade_indicator_destructibles);
        // ---- batch 17: menus ----
        switch!(app, "keep-acting-notes", "Keep Acting Notes", "Spoken lines keep their {acting notes} in the subtitles. (Stewie's Tweaks bDialogueKeepVoiceActingNotes)", game.keep_acting_notes);
        switch!(app, "hide-completed-quests", "Hide Finished Quests", "The Pip-Boy lists only quests under way. (Stewie's Tweaks bHideCompletedQuests)", game.hide_completed_quests);
        switch!(app, "hide-marker-reputation", "No Reputation on Map Markers", "Map markers show no reputation line. (Stewie's Tweaks bHideMapMarkerFactionReputation)", game.hide_marker_reputation);
        switch!(app, "no-show-quest-location", "No Show Location", "The quests page has no Show Location button. (Stewie's Tweaks bDisableShowQuestLocation)", game.no_show_quest_location);
        switch!(app, "hide-trait-ranks", "No Ranks for Traits", "The trait menu shows no Ranks line. (Stewie's Tweaks bHideRanksInTraitMenu)", game.hide_trait_ranks);
        switch!(app, "hide-quest-item-figures", "No Figures for Quest Items", "Quest items and keys show no weight or value in the Pip-Boy. (Stewie's Tweaks bHideQuestItemWeightAndValue)", game.hide_quest_item_figures);
        switch!(app, "single-item-figures", "One Item's Figures", "A stack shows one item's weight and value in the Pip-Boy. (Stewie's Tweaks bIndividialItemStats)", game.single_item_figures);
        switch!(app, "mark-other-ammo", "Mark Other Ammo", "A weapon's ammo line ends with + when another ammo it takes is carried. (Stewie's Tweaks bShowAlternateAmmoTypesAvailableInMenus)", game.mark_other_ammo);
        switch!(app, "show-fame-infamy", "Show Fame and Infamy", "Reputations show their fame and infamy. (Stewie's Tweaks bReputationShowsFameInfamy)", game.show_fame_infamy);
        switch!(app, "take-all-confirm", "Take All Confirmation", "Take All asks first; press it again to take everything. (Stewie's Tweaks bTakeAllConfirmation)", take_all_confirm);
        amount!(app, "take-all-confirm-min", "Take All Confirmation: From This Many", "Take All asks only when the container holds at least this many items. (Stewie's Tweaks bTakeAllConfirmation, iMinItemCount)", 100.0, f32, take_all_confirm_min);
        switch!(app, "repair-all-confirm", "Repair All Confirmation", "Repair All at a vendor asks first; press it again to pay. (Stewie's Tweaks bRepairAllConfirmation)", repair_all_confirm);
        amount!(app, "repair-all-threshold", "Repair All Confirmation: From This Price", "Repair All asks only when it costs at least this many caps. (Stewie's Tweaks bRepairAllConfirmation, iPriceThreshold)", 5000.0, f32, repair_all_threshold);
        switch!(app, "take-all-stays", "Take All Stays Open", "Take All leaves the container open. (Stewie's Tweaks bContainerTakeAllDoesntCloseMenu)", take_all_stays);
        switch!(app, "hide-equipped-barter", "Hide Worn Items in Barter", "Equipped items are not offered in barter. (Stewie's Tweaks bHideEquippedItemsInBarter)", hide_equipped_barter);
        switch!(app, "hide-equipped-containers", "Hide Worn Items in Containers", "Equipped items are not listed on the player's side of a container. (Stewie's Tweaks bHideEquippedItemsInContainers)", hide_equipped_containers);
        switch!(app, "title-case-categories", "Quiet Category Titles", "Container category titles in title case, not capitals. (Stewie's Tweaks bNoCapitaliseContainerCategories)", title_case_categories);
        switch!(app, "no-right-click-drop", "No Right-Click Drop", "A right click in the Pip-Boy inventory does not drop the item. (Stewie's Tweaks bDisableInventoryRightClickDrop)", no_right_click_drop);
        switch!(app, "quiet-undiscovered", "Quiet Undiscovered Markers", "Clicking an undiscovered map marker puts up no box. (Stewie's Tweaks bPreventNoFastTravelMessage)", quiet_undiscovered);
        // ---- batch 19 ----
        switch!(app, "repair-any-armor", "Repairers Take Any Armor", "A repairer lists armor with no DT too. (Stewie's Tweaks bShowQuestAndNoDRDTItemsInRepairMenu)", game.repair_any_armor);
        switch!(app, "sort-misc-stats", "Sorted Statistics", "The STATS page's misc statistics in name order. (Stewie's Tweaks bSortMiscStats)", game.sort_misc_stats);
        switch!(app, "effect-time-left", "Effect Time Left", "Effects on the STATS page show the seconds left. (Stewie's Tweaks bStatsMenuShowEffectTimeRemaining)", game.effect_time_left);
        switch!(app, "perk-plugin-names", "Perk Plugins", "A level-up perk's description names the plugins it comes from. (Stewie's Tweaks bShowModNameInPerkDescriptions)", game.perk_plugin_names);
        amount!(app, "item-added-seconds", "Item Added Line Time", "Seconds an item-added line stays; 0 changes nothing. (Stewie's Tweaks bCustomAddToInventoryMessageTimer)", 10.0, f32, game.item_added_seconds);
        switch!(app, "pipboy-12-hour", "12-Hour Pip-Boy Clock", "The Pip-Boy's clock in 12 hours with AM and PM. (Stewie's Tweaks b12HourPipboyClock)", game.pipboy_12_hour);
        // ---- batch 20 ----
        switch!(app, "notes-of-all-quests", "Notes of Every Quest", "Show Active Quest Notes takes in every quest under way. (Stewie's Tweaks bShowActiveQuestNotesShowsAllStartedQuests)", game.notes_of_all_quests);
        part!(app, "aim-turn-x", "Aiming Turn Speed: Across", "How fast the view turns left and right while aiming, as a part of the usual; 100% changes nothing. (Stewie's Tweaks bTurnSlowerWhileAiming)", aim_turn_x);
        part!(app, "aim-turn-y", "Aiming Turn Speed: Up and Down", "How fast the view turns up and down while aiming, as a part of the usual; 100% changes nothing. (Stewie's Tweaks bTurnSlowerWhileAiming)", aim_turn_y);
        switch!(app, "pickpocket-after-caught", "Pickpocket Again", "Someone who caught the player pickpocketing can be tried again. (Stewie's Tweaks bAllowPickpocketIfAlreadyCaught)", game.pickpocket_after_caught);
        switch!(app, "wait-anywhere", "Wait Anywhere", "Only a script's ban stops waiting or sleeping: not combat, enemies, water, falling or someone's bed. (Stewie's Tweaks bSleepWaitAnywhere)", game.wait_anywhere);
        part!(app, "radio-music", "Radio Songs Volume", "The radio's songs, as a part of their volume; 100% changes nothing. (Stewie's Tweaks bScaleRadioVolume)", radio_music_volume);
        part!(app, "radio-talk", "Radio Talk Volume", "The radio's talk, as a part of its volume; 100% changes nothing. (Stewie's Tweaks bScaleRadioVolume)", radio_talk_volume);
        switch!(app, "camera-wheel-pov", "Wheel Zoom Needs Toggle View", "The mouse wheel moves the third-person camera only while Toggle View is held. (Stewie's Tweaks bOnlyChangeCameraHeightIfPOVKeyHeld)", camera_wheel_needs_pov);
        switch!(app, "quiet-hotkeys", "No Hotkey Sounds", "Weapon hotkeys make no equip sound. (Stewie's Tweaks bNoHotkeyEquipSounds)", quiet_hotkeys);
        // ---- batch 21 ----
        switch!(app, "detached-beams", "Detached Beams", "The player's beams stay where they were fired instead of following the barrel. (Stewie's Tweaks bDetachedBeams)", game.detached_beams);
        switch!(app, "god-mode-off-on-load", "God Mode Off on Load", "Loading a save turns god mode off. (Stewie's Tweaks bDisableGodmodeOnLoad)", game.god_mode_off_on_load);
        switch!(app, "no-stealth-shimmer-pipboy", "No Stealth Shimmer in the Pip-Boy", "The stealth shimmer is off while the Pip-Boy is up. (Stewie's Tweaks bDisableStealthEffectInPipboy)", game.no_stealth_shimmer_in_pipboy);
        switch!(app, "quiet-teammate-steps", "Quiet Companion Footsteps", "Teammates' footsteps make no sound. (Stewie's Tweaks bPreventTeammateFootstepSounds)", game.quiet_teammate_steps);
        switch!(app, "no-weapon-hotkeys", "No Weapon Hotkeys", "The number keys and the D-pad pick no weapon (the wheel still does). (Stewie's Tweaks bDisableWeaponHotkeys)", no_weapon_hotkeys);
        switch!(app, "pipboy-opens-items", "Pip-Boy Opens on Items", "The Pip-Boy always opens on the inventory. (Stewie's Tweaks bOpenPipboyToInventoryByDefault)", pipboy_opens_items);
        switch!(app, "hide-crosshair-reloading", "No Crosshair While Reloading", "The crosshair is hidden while reloading. (Stewie's Tweaks bHideCrosshairWhileReloading)", hide_crosshair_reloading);
        // ---- batch 22 ----
        add_setting(
            app,
            Setting::slider(
                "tweak-kill-xp-per-level",
                GROUP,
                "Kill XP by Level",
                "Kill experience gains this per level the target is above you, loses it below; 0% is off. (Stewie's Tweaks bLevelDifferenceAffectsCombatXP)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| (t.game.kill_xp_per_level * 5.0) as f32),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.kill_xp_per_level = f64::from(v) / 5.0;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.0}%", v * 20.0)),
                },
            ),
        );
        part!(app, "key-xp-share", "Experience for Keys", "Opening a lock with its key gives this share of picking it; 0% changes nothing. (Stewie's Tweaks bUsingKeysRewardsXP)", game.key_xp_share);
        part!(app, "password-xp-share", "Experience for Passwords", "Opening a terminal with its password gives this share of hacking it, once; 0% changes nothing. (Stewie's Tweaks bUsingNotesRewardsXP)", game.password_xp_share);
        // ---- batch 23 ----
        switch!(app, "invert-x", "Invert Look Left/Right", "The mouse and the stick turn the view the other way left and right. (Stewie's Tweaks bInvertCameraX)", invert_x);
        switch!(app, "nearest-unfound", "Nearest Unfound Place", "The nearest place not yet found shows on the compass at any distance. (Stewie's Tweaks bShowNearestUndiscoveredLocation)", nearest_unfound_on_compass);
        switch!(app, "discovered-in-corner", "Discoveries in the Corner", "A place found is a corner line, not a banner. (Stewie's Tweaks bLocationDiscoveredCornerMessage)", discovered_in_corner);
        // ---- batch 24 ----
        switch!(app, "power-armor-radiation", "Power Armor Blocks Radiation", "Power armor on the body keeps the world's radiation off; water and chems still count. (Stewie's Tweaks bNoEnvironmentRadiationInFullPowerArmor)", game.power_armor_blocks_radiation);
        add_setting(
            app,
            Setting::slider(
                "tweak-power-armor-limbs",
                GROUP,
                "Limb Damage in Power Armor",
                "Limb damage to anyone in power armor is multiplied by this; x1.0 changes nothing. (Stewie's Tweaks bPowerArmorScalesLimbDamage)",
                Slider {
                    get: |w| part(w.get_resource::<TweakSettings>().map_or(1.0, |t| t.game.power_armor_limb_damage)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.power_armor_limb_damage = factor(v);
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(shown_factor),
                },
            ),
        );
        add_setting(
            app,
            Setting::slider(
                "tweak-fall-height-endurance",
                GROUP,
                "Endurance Softens Falls",
                "Safe fall height gains this per Endurance over 5, and loses it below; 0% changes nothing. (Stewie's Tweaks bEnduranceScalesMinFallHeight)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| t.game.fall_height_per_endurance * 5.0),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.fall_height_per_endurance = v / 5.0;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.0}%", v * 20.0)),
                },
            ),
        );
        // ---- batch 25 ----
        switch!(app, "invert-title-wheel", "Reverse Category Wheel", "The mouse wheel over a container's category title steps the other way. (Stewie's Tweaks bInvertContainerTitleScrollwheelDirection)", invert_title_wheel);
        switch!(app, "armor-condition", "Armor Condition on the HUD", "The HUD shows the condition of the armour worn on the body, under the weapon's. (Stewie's Tweaks bArmorConditionLabel)", game.armor_condition);
        part!(app, "armor-visible-below", "Armor Condition: Shown Below", "The armour's row shows only below this condition; 0% always. (Stewie's Tweaks bArmorConditionLabel)", armor_visible_below);
        part!(app, "armor-red-below", "Armor Condition: Red Below", "The armour's bar turns red below this condition; 0% never. (Stewie's Tweaks bArmorConditionLabel)", armor_red_below);
        part!(app, "armor-flash-below", "Armor Condition: Flashing Below", "The armour's bar flashes red below this condition; 0% never. (Stewie's Tweaks bArmorConditionLabel)", armor_flash_below);
        switch!(app, "armor-in-weapon-place", "Armor Condition: In the Weapon's Place", "With no weapon drawn the armour's row takes the weapon's place. (Stewie's Tweaks bArmorConditionLabel)", armor_in_weapon_place);
        switch!(app, "scale-explosion-shake", "Scale Explosion Shake", "The view shakes more or less when an explosion goes off near the player. (Stewie's Tweaks bScaleExplosionShake)", scale_explosion_shake);
        amount!(app, "explosion-shake", "Scale Explosion Shake: Camera", "How strongly the view shakes, 1 as it is, 0 not at all. (Stewie's Tweaks bScaleExplosionShake, fCameraMult)", 5.0, f32, explosion_shake);
        switch!(app, "toggle-device", "Switch Device by Pressing", "Pressing A on a controller switches the controls to it; a left click switches back to the keyboard and mouse. (Stewie's Tweaks bToggleControllerIfAttackPressed)", toggle_device);
        switch!(app, "hud-fatigue", "Fatigue Indicator", "An FTG word beside the HUD's H20 and FOD shows while Fatigue is below full: red below half, flashing below a quarter. (Stewie's Tweaks bHUDFatigueIndicator)", hud_fatigue);
        switch!(app, "key-repeat-accel", "Key Repeat Acceleration", "A key held down in a menu repeats faster the longer it is held. (Stewie's Tweaks bKeyRepeatAcceleration)", key_repeat_accel);
        amount!(app, "key-repeat-delay", "Key Repeat Acceleration: First Delay", "Seconds held before the repeats speed up the first time. (Stewie's Tweaks bKeyRepeatAcceleration, iDelay)", 5.0, f32, key_repeat_delay);
        amount!(app, "key-repeat-mult", "Key Repeat Acceleration: First Speed", "How many times as fast the repeats come after the first delay. (Stewie's Tweaks bKeyRepeatAcceleration, fRateMult)", 10.0, f32, key_repeat_mult);
        amount!(app, "key-repeat-delay-alt", "Key Repeat Acceleration: Second Delay", "Seconds held before the repeats speed up again. (Stewie's Tweaks bKeyRepeatAcceleration, iDelayAlt)", 10.0, f32, key_repeat_delay_alt);
        amount!(app, "key-repeat-mult-alt", "Key Repeat Acceleration: Second Speed", "How many times as fast the repeats come after the second delay. (Stewie's Tweaks bKeyRepeatAcceleration, fRateMultAlt)", 10.0, f32, key_repeat_mult_alt);
        switch!(app, "precise-rad-meter", "More Precise RAD Meter", "The HUD's radiation meter shows its rads a second with decimal places. (Stewie's Tweaks bMorePreciseRadMeter)", precise_rad_meter);
        amount!(app, "rad-decimals", "More Precise RAD Meter: Decimal Places", "How many places after the point the meter shows (1 to 9). (Stewie's Tweaks bMorePreciseRadMeter, sFormat)", 9.0, u8, rad_decimals);
        switch!(app, "compass-doors", "Doors on the Compass", "Doors leading somewhere else show on the compass, as the local map draws them. (Stewie's Tweaks bShowDoorsOnCompass)", compass_doors);
        amount!(app, "compass-doors-exterior", "Doors on the Compass: Reach Outside", "How far away a door outside still shows, in game units (70 a metre). (Stewie's Tweaks bShowDoorsOnCompass)", 10000.0, f32, compass_doors_exterior);
        amount!(app, "compass-doors-interior", "Doors on the Compass: Reach Inside", "How far away a door inside still shows, in game units (70 a metre). (Stewie's Tweaks bShowDoorsOnCompass)", 5000.0, f32, compass_doors_interior);
        switch!(app, "compass-doors-fade", "Doors on the Compass: Fade by Distance", "A door's mark is fainter the further away it is. (Stewie's Tweaks bShowDoorsOnCompass)", compass_doors_fade);
        switch!(app, "detected-by-whom", "Detected by Whom", "The sneak word [SPOTTED] names who has spotted the player. (Stewie's Tweaks bDetectedByWhom)", detected_by_whom);
        amount!(app, "detected-names", "Detected by Whom: Names Shown", "How many names come before \"and N others\"; 0 shows only how many. (Stewie's Tweaks bDetectedByWhom)", 5.0, f32, detected_names);
        switch!(app, "quest-text-in-menus", "Quest Text Over the Pip-Boy", "The quest and location lines stay on screen, and go on, while the Pip-Boy or a terminal is open. (Stewie's Tweaks bQuestTextVisibleInMenus)", quest_text_in_menus);
        switch!(app, "continue-shows-save", "Continue Shows Its Save", "On the main menu, the picture of the save Continue loads is shown while Continue is chosen. (Stewie's Tweaks bMainMenuContinueIcon)", continue_shows_save);
        switch!(app, "auto-continue", "Continue at Start", "The main menu continues the newest save at once, the first time it opens; hold Shift to stay on the menu. (Stewie's Tweaks bAutoContinueGame)", auto_continue);
        // ---- batch 26 ----
        switch!(app, "hide-ammo-label", "No Ammo Counter", "The HUD shows no ammo counter. (Stewie's Tweaks bHideAmmoLabel)", hide_ammo_label);
        switch!(app, "barter-by-kind", "Barter Prices by Kind", "Prices in a barter are multiplied by the kind of item and by whether the player sells or buys it. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", barter_by_kind);
        switch!(app, "barter-water-unscaled", "Barter by Kind: Purified Water Unscaled", "Purified water keeps its price whatever the Aid multipliers. (Stewie's Tweaks bUseCustomBarterPriceMultipliers, bConstantPurifiedWaterPrice)", barter_water_unscaled);
        amount!(app, "barter-sell-armor", "Barter by Kind: Selling Armor", "The price multiplier of armor items the player sells, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_sell_armor);
        amount!(app, "barter-sell-weapon", "Barter by Kind: Selling Weapon", "The price multiplier of weapon items the player sells, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_sell_weapon);
        amount!(app, "barter-sell-aid", "Barter by Kind: Selling Aid", "The price multiplier of aid items the player sells, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_sell_aid);
        amount!(app, "barter-sell-ammo", "Barter by Kind: Selling Ammo", "The price multiplier of ammo items the player sells, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_sell_ammo);
        amount!(app, "barter-sell-misc", "Barter by Kind: Selling Misc", "The price multiplier of misc items the player sells, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_sell_misc);
        amount!(app, "barter-sell-weapon-mod", "Barter by Kind: Selling Weapon Mod", "The price multiplier of weapon mod items the player sells, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_sell_weapon_mod);
        amount!(app, "barter-buy-armor", "Barter by Kind: Buying Armor", "The price multiplier of armor items the player buys, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_buy_armor);
        amount!(app, "barter-buy-weapon", "Barter by Kind: Buying Weapon", "The price multiplier of weapon items the player buys, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_buy_weapon);
        amount!(app, "barter-buy-aid", "Barter by Kind: Buying Aid", "The price multiplier of aid items the player buys, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_buy_aid);
        amount!(app, "barter-buy-ammo", "Barter by Kind: Buying Ammo", "The price multiplier of ammo items the player buys, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_buy_ammo);
        amount!(app, "barter-buy-misc", "Barter by Kind: Buying Misc", "The price multiplier of misc items the player buys, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_buy_misc);
        amount!(app, "barter-buy-weapon-mod", "Barter by Kind: Buying Weapon Mod", "The price multiplier of weapon mod items the player buys, before the Barter skill's. (Stewie's Tweaks bUseCustomBarterPriceMultipliers)", 5.0, f32, barter_buy_weapon_mod);
        amount!(app, "free-item-price", "Price for Worthless Things", "What merchants pay or ask for things worth nothing (spent casings); 0 changes nothing. (Stewie's Tweaks bNoFreeBarterItems)", 10.0, f32, free_item_price);
        // ---- batch 27 ----
        switch!(app, "skip-easy-locks", "Skip Easy Locks", "A lock well under the player's Lockpick opens at once (with its experience); use it again to go through. (Stewie's Tweaks bSkipVeryEasyLocksAtMaxSkill)", game.skip_easy_locks);
        amount!(app, "skip-lock-margin", "Skip Easy Locks: Margin", "How far over the lock Lockpick must be to skip the game (0 to 50). (Stewie's Tweaks bSkipVeryEasyLocksAtMaxSkill, iThreshold)", 50.0, f32, game.skip_lock_margin);
        switch!(app, "pick-up-disarmed", "Pick Up Disarmed Mines", "A mine the player disarms goes straight into the inventory. (Stewie's Tweaks bDisarmingMinesPicksThemUp)", game.pick_up_disarmed);
        // ---- batch 28 ----
        switch!(app, "npc-weapons-whole", "NPC Weapons Hit Whole", "NPCs' weapons hit as hard as whole ones, whatever their condition. (Stewie's Tweaks bNoScaleNpcDamageByCondition)", game.npc_weapons_whole);
        switch!(app, "lethal-knockdowns", "Killing Knockdowns Count", "A knockdown that kills still counts for the knockdown challenges. (Stewie's Tweaks bLethalHitsIncreaseLimbChallenges)", game.lethal_knockdowns_count);
        switch!(app, "duplicate-binds", "Shared Keys", "Binding an input to a control leaves other controls with it as they are (no swap). (Stewie's Tweaks bAllowDuplicateControlBinds)", duplicate_binds);
        // ---- batch 29 ----
        switch!(app, "crippling-never-disarms", "Crippling Never Disarms", "A crippled arm never makes anyone drop their weapon. (Stewie's Tweaks bCripplingDoesntDisarm)", game.crippling_never_disarms);
        switch!(app, "no-spin-up", "No Spin-Up", "The player's automatic weapons fire without spinning up first. (Stewie's Tweaks bAutoWeaponNoFiringDelay)", game.no_spin_up);
        switch!(app, "god-mode-free-crafting", "Free Crafting in God Mode", "In god mode recipes need no parts or skill. (Stewie's Tweaks bGodModeAllowsCraftingWithoutRequirements)", game.god_mode_free_crafting);
        switch!(app, "pov-follows-weapon", "View Follows the Weapon", "Drawing the weapon goes to first person, holstering it to third. (Stewie's Tweaks bTogglePOVWhenHolsteringWeapon)", pov_follows_weapon);
        part!(app, "kill-cam-chance", "Kill Camera Chance", "The share of kill cameras that play (after the Kill Camera setting's own choice); 100% changes nothing. (Stewie's Tweaks bChanceBasedKillcams)", kill_cam_chance);
        amount!(app, "subtitle-distance", "Subtitle Distance", "General subtitles show for speakers within this many units; 0 changes nothing. (Stewie's Tweaks bCustomSubtitleDistance)", 10000.0, f32, game.subtitle_distance);
        switch!(app, "sneak-criticals-head", "Sneak Criticals Head Only", "Sneak attacks are criticals only on the head. (Stewie's Tweaks bSneakCriticalsHeadshotsOnly)", game.sneak_criticals_head_only);
        switch!(app, "compare-weapons", "Compare Weapons", "A weapon's DAM and DPS in the Pip-Boy show the difference from the weapon in hand (both whole). (Stewie's Tweaks bCompareWeaponStats)", game.compare_weapons);
        switch!(app, "warn-encumbrance", "Weight Warning", "In a container, the player's weight shows red when taking the selected item would be too much to carry. (Stewie's Tweaks bContainerEncumbranceIndicator)", warn_encumbrance);
        amount!(app, "mark-nearby-places", "Mark Nearby Places", "Map markers within this many units of the player are marked on the map; 0 changes nothing. (Stewie's Tweaks bMarkNearbyLocationsOnMap)", 10000.0, f32, game.mark_nearby_places);
        switch!(app, "scale-ash-piles", "Ash Piles Fit the Body", "An ash pile takes the size of the actor it was (between half and twice). (Stewie's Tweaks bScaleAshpileSize)", game.scale_ash_piles);
        switch!(app, "pipboy-buzz", "Pip-Boy Buzz", "A short controller buzz on each Pip-Boy page or tab change (with rumble on). (Stewie's Tweaks bHapticPipboy)", pipboy_buzz);
        switch!(app, "per-bullet-reload", "Per-Bullet Looping Reloads", "A shotgun, revolver or other round-at-a-time weapon loads as many rounds as Reload is pressed for. (Stewie's Tweaks bPerBulletLoopingReloads)", game.per_bullet_reload);
        amount!(app, "reload-rounds-per-press", "Per-Bullet Reloads: Rounds a Press", "Rounds each Reload press loads. (Stewie's Tweaks bPerBulletLoopingReloads)", 5.0, u32, game.reload_rounds_per_press);
        amount!(app, "reload-press-queue", "Per-Bullet Reloads: Most Queued", "The most rounds the presses can queue; 0 for no most. (Stewie's Tweaks bPerBulletLoopingReloads)", 20.0, u32, game.reload_press_queue);
        switch!(app, "reload-while-firing", "Reload While Firing", "A reload can start while the last shot's recovery still runs. (Stewie's Tweaks bReloadWhileFiring)", game.reload_while_firing);
        add_setting(
            app,
            Setting::slider(
                "tweak-semi-auto-queue",
                GROUP,
                "Semi-Auto Shot Queue",
                "A semi-automatic pull this close before the weapon is ready again is kept and fired then (0 to 0.5 s); 0 changes nothing. (Stewie's Tweaks bSemiAutoQueue)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| t.game.semi_auto_queue * 2.0),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.semi_auto_queue = v / 2.0;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.3} s", v / 2.0)),
                },
            ),
        );
        switch!(app, "repair-scales-crafting", "Crafting Uses Repair", "Weapons and armour made at a workbench come out at a condition set by Repair: half at 0, whole at 100. (Stewie's Tweaks bRepairScalesCraftingCondition)", game.repair_scales_crafting);
        amount!(app, "crafted-item-health-pct", "Crafted Item Condition", "Percent condition of weapons and armour made at a workbench; 0 leaves it unchanged. (Stewie's Tweaks fCraftedItemHealthPct)", 100.0, f32, game.crafted_item_health_pct);
        switch!(app, "no-healing-in-combat", "No Healing In Combat", "Aid that restores health or a limb cannot be used from the Pip-Boy while something fights the player. (Stewie's Tweaks bNoHealingInCombat)", game.no_healing_in_combat);
        switch!(app, "power-attack-if-blocking", "Power Attack From a Block", "While blocking with a melee weapon, a press of attack is a power attack at once, with no hold. (Stewie's Tweaks bPowerAttackIfBlocking)", power_attack_if_blocking);
        amount!(app, "breath-regain", "Slow Breath Regen", "Seconds of breath each second at the surface gives back; the meter stays up until it is whole. 0 refills it at once. (Stewie's Tweaks bSlowBreathRegen)", 20.0, f32, game.breath_regain);
        switch!(app, "vats-hip-fire", "V.A.T.S. Hip Fire", "V.A.T.S. entered without aiming fires every shot from the hip (the chance to hit is the same). (Stewie's Tweaks bVATSHipFire)", vats_hip_fire);
        switch!(app, "pickpocket-caught-detects", "Caught Pickpocketing Means Detected", "Catching the player pickpocketing detects them: a hostile actor no longer leaves them hidden. (Stewie's Tweaks bPickpocketCaughtCausesDetection)", game.pickpocket_caught_detects);
        add_setting(
            app,
            Setting::slider(
                "tweak-agility-jump",
                GROUP,
                "Agility Scales Jump Height",
                "The jump height times 1 + this x (Agility - 1), from 0 to 0.2 per point; 0 changes nothing. (Stewie's Tweaks bAgilityScalesJumpHeight)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| t.game.agility_jump_mult * 5.0),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.agility_jump_mult = v / 5.0;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.2}", v / 5.0)),
                },
            ),
        );
        amount!(app, "lockpick-force-near", "Force Lock Near the Sweet Spot", "Percent added to Force Lock at the sweet spot, less further off, none at 90 degrees; 0 is off. (Stewie's Tweaks bLockpickForceBonusNearSweetSpot)", 50.0, f32, game.lockpick_force_near_bonus);
        switch!(app, "contact-mines-player", "Contact Mines (Player)", "A mine the player stands on goes off at once, with no fuse. (Stewie's Tweaks bContactMines, bPlayer)", game.contact_mines_player);
        switch!(app, "contact-mines-npcs", "Contact Mines (Others)", "A mine anyone else stands on goes off at once, with no fuse. (Stewie's Tweaks bContactMines, bNPCs)", game.contact_mines_npcs);
        amount!(app, "sleep-heal-min-hours", "Sleep Healing Min Duration", "Hours a sleep must last before it restores Health and heals limbs; 0 heals from the first hour. (Stewie's Tweaks bSleepHealingMinDuration)", 12.0, u32, game.sleep_heal_min_hours);
        switch!(app, "sleep-misc-stat-requires-duration", "Sleep Healing: Stat Needs Full Duration", "Times Slept counts a sleep only when it meets Sleep Healing Min Duration. (Stewie's Tweaks bSleepHealingMinDuration, bMiscStatRequiresSleepDuration)", game.sleep_misc_stat_requires_duration);
        amount!(app, "barter-repair-discount", "Barter Lowers Repair Costs", "Percent of a vendor's repair price the player's Barter takes off at 100 (in proportion below); 0 changes nothing. (Stewie's Tweaks bBarterAffectsRepairCosts)", 60.0, f32, game.barter_repair_discount);
        add_setting(
            app,
            Setting::slider(
                "tweak-armor-condition-penalty",
                GROUP,
                "Armour Condition Penalty",
                "How fast armour under half condition loses DT and DR (x1.0: to half, the original; x2.0: to none). (Stewie's Tweaks bCustomArmorConditionPenalty)",
                Slider {
                    get: |w| part(w.get_resource::<TweakSettings>().map_or(1.0, |t| t.game.armor_condition_penalty)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.armor_condition_penalty = factor(v);
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(shown_factor),
                },
            ),
        );
        switch!(app, "vats-ignores-companions", "V.A.T.S. Ignores Companions", "Companions cannot be targeted in V.A.T.S. (Stewie's Tweaks bIgnoreCompanionsVATS)", game.vats_ignores_companions);
        switch!(app, "no-pipboy-in-combat", "No Pip-Boy In Combat", "The Pip-Boy does not open while something fights the player; a corner message says so. (Stewie's Tweaks bNoPipBoyInCombat)", no_pipboy_in_combat);
        switch!(app, "frenzy-kill-xp", "Frenzied Kills Earn Experience", "A kill by a frenzied actor (Aggression Frenzied) earns the player experience. (Stewie's Tweaks bFrenzyGrantsPlayerXP)", game.frenzy_kill_xp);
        switch!(app, "free-wild-wasteland", "Free Wild Wasteland", "Wild Wasteland can be taken without using one of the trait choices. (Stewie's Tweaks bFreeWildWasteland)", game.free_wild_wasteland);
        switch!(app, "stats-xp-percent", "Experience as a Percentage", "The Stats page shows experience as how far through the level the player is, in percent. (Stewie's Tweaks bStatsMenuPercentageXP)", game.stats_xp_percent);
        switch!(app, "repair-same-first", "Repair List: Same Item First", "In the Pip-Boy repair list, parts that are the same item come first, the rest by name. (Stewie's Tweaks bSortPipboyRepairMenu)", game.repair_same_first);
        switch!(app, "repair-low-first", "Repair List: Low Condition First", "In the Pip-Boy repair list, parts in worse condition come first and the best at the bottom. (Stewie's Tweaks bInvertPipboyRepairMenuSorting)", game.repair_low_first);
        switch!(app, "repair-kits-in-list", "Repair Kits in the Repair List", "A weapon's Pip-Boy repair list also offers Weapon Repair Kits, working as a kit does. (Stewie's Tweaks bUseWeaponRepairKitsInRepairMenu)", game.repair_kits_in_list);
        switch!(app, "repair-preview", "Repair Items Preview", "Alt with R in the Pip-Boy's items previews what could repair the item, even at full; nothing is used. (Stewie's Tweaks bRepairItemsPreview)", game.repair_preview);
        amount!(app, "crippled-fall-pain", "Crippled Legs: Pain When Falling", "Falls this high on a crippled leg make you cry out; 0 is off. (Stewie's Tweaks bCrippledLimbsPlayPainSoundWhenFalling, fHeightThreshold)", 1000.0, f32, game.crippled_fall_pain);
        switch!(app, "quantity-companion-cap", "Quantity Menu: Companion Carry Cap", "Giving a stack to a companion, the quantity menu goes no higher than what they can still carry. (Stewie's Tweaks bQuantityMenuRespectsCompanionCarryCap)", game.quantity_companion_cap);
        switch!(app, "armor-blocks-blood", "Armor Prevents Blood Decals", "A shot whose damage does not get through the target's Damage Threshold leaves no blood on them. (Stewie's Tweaks bArmorPreventsBloodDecals)", game.armor_blocks_blood);
        switch!(app, "embolden-tags", "Tag Skills in Bold", "The Pip-Boy's Skills list shows your tag skills in bold. (Stewie's Tweaks bEmboldenTagSkills)", game.embolden_tags);
        switch!(app, "pickpocket-overhaul", "Pickpocket Overhaul", "Pickpocket odds count item weight, the target's Perception and sight, your Agility, Luck and Sneak. (Stewie's Tweaks bPickpocketOverhaul)", game.pickpocket_overhaul);
        amount!(app, "pp-base", "Pickpocket Overhaul: Base Chance", "The chance before the figures, in percent. (Stewie's Tweaks bPickpocketOverhaul, fBaseChance)", 100.0, f32, game.pp_base);
        amount!(app, "pp-agility", "Pickpocket Overhaul: Agility", "Tenths of a percent per point of your Agility. (fPlayerAgilityMult)", 50.0, f32, game.pp_agility);
        amount!(app, "pp-luck", "Pickpocket Overhaul: Luck", "Tenths of a percent per point of your Luck. (fPlayerLuckMult)", 50.0, f32, game.pp_luck);
        amount!(app, "pp-sneak", "Pickpocket Overhaul: Sneak", "Tenths of a percent per point of your Sneak. (fPlayerSneakMult)", 50.0, f32, game.pp_sneak);
        amount!(app, "pp-perception", "Pickpocket Overhaul: Target Perception", "Tenths of a percent off per point of the target's Perception. (fTargetPerceptionMult)", 100.0, f32, game.pp_perception);
        amount!(app, "pp-detection", "Pickpocket Overhaul: Being Seen", "Tenths of a percent off per point of how well the target detects you (0 to 100). (fDetectionValueMult)", 100.0, f32, game.pp_detection);
        amount!(app, "pp-value", "Pickpocket Overhaul: Item Value", "Hundredths of a percent off per cap the items are worth. (fItemValueMult)", 50.0, f32, game.pp_value);
        amount!(app, "pp-weight", "Pickpocket Overhaul: Item Weight", "Tenths of a percent off per pound the items weigh. (fItemWeightMult)", 100.0, f32, game.pp_weight);
        amount!(app, "pp-worn", "Pickpocket Overhaul: Worn Items", "A worn item's chance times this many tenths. (fWornItemChanceMult)", 50.0, f32, game.pp_worn);
        switch!(app, "pp-free-items", "Pickpocket Overhaul: Free Items Always", "Items worth nothing, such as keys, are always taken unseen. (bIgnoreFreeItems)", game.pp_free_items);
        switch!(app, "pp-reward-xp", "Pickpocket Overhaul: Experience", "A pocket picked gives experience: the items' weight and value by their figures above, plus the target's Perception. (bRewardXP)", game.pp_reward_xp);
        switch!(app, "hold-activate", "Hold to Activate", "Some things need the Activate key held half a second instead of pressed (chosen below). (Stewie's Tweaks bHoldToActivate)", game.hold_activate);
        switch!(app, "hold-containers", "Hold to Activate: Containers and Bodies", "Searching containers and bodies needs the hold. (bContainers)", game.hold_containers);
        switch!(app, "hold-furniture", "Hold to Activate: Furniture", "Sitting, sleeping and other furniture need the hold. (bFurniture)", game.hold_furniture);
        switch!(app, "hold-water", "Hold to Activate: Water", "Drinking from a tap or puddle needs the hold. (bWater)", game.hold_water);
        switch!(app, "quick-use", "Quick Use", "Shift-taking an item uses it: a weapon goes in hand, aid is taken; armour is only taken. (Stewie's Tweaks bQuickUse, bRealtimeQuickUse)", game.quick_use);
        switch!(app, "vats-throw-visibility", "V.A.T.S. Throwables Use Visibility", "Thrown weapons (not grenades): a V.A.T.S. part's chance is times how much of it is in sight. (Stewie's Tweaks bVATSThrowablesUseVisibility)", game.vats_throw_visibility);
        switch!(app, "quiet-followers", "Quiet Companions", "Companions do not call out that they are hurt, crippled or out of ammo. (Stewie's Tweaks bRemoveFollowerTopics)", game.quiet_followers);
        switch!(app, "crit-message-limb", "Critical Hit Messages Name the Limb", "The critical-hit message names the part you struck, as \"Critical hit on Easy Pete's torso\". (Stewie's Tweaks bCriticalHitMessagesIncludeLimbName)", game.crit_message_limb);
        switch!(app, "no-dehydration-messages", "No Dehydration Messages", "No corner message when your dehydration gets better or worse. (Stewie's Tweaks bDisableNeedsMessages, bDehydration)", game.no_dehydration_messages);
        switch!(app, "no-sleep-messages", "No Sleep Deprivation Messages", "No corner message when your sleep deprivation gets better or worse. (Stewie's Tweaks bDisableNeedsMessages, bSleep)", game.no_sleep_messages);
        switch!(app, "need-icons", "Custom Popup Icons", "The dehydration and sleep messages carry a picture: the game's water, and its Healing Rate for sleep. (Stewie's Tweaks bCustomPopupIcons)", game.need_icons);
        switch!(app, "repair-at-workbench", "Repair Only at a Workbench", "The Pip-Boy repairs items only when you opened it looking at a workbench. (Stewie's Tweaks bPreventRepairIfNotAtWorkbench)", game.repair_at_workbench);
        switch!(app, "character-radiation", "Character Radiation", "People and creatures take the radiation where they stand, as you do. (Stewie's Tweaks bCharacterRadiation)", game.character_radiation);
        switch!(app, "character-radiation-stages", "Character Radiation: Sickness Too", "With Character Radiation, their rads bring on radiation sickness's effects as yours do. (Stewie's Tweaks bCharacterRadiation, bAllowRadiationStages)", game.character_radiation_stages);
        switch!(app, "reallocate-skill-points", "Reallocate Skill Points on Level-Up", "In the level-up menu, the minus also takes back points you put into a skill at earlier levels, to spend again. (Stewie's Tweaks bReallocateSkillPointsOnLevelup)", game.reallocate_skill_points);
        switch!(app, "local-map-reset-cells", "Local Map Shows Reset Areas", "Outdoors the local map tints red what resets when you next come near (people, containers). (Stewie's Tweaks bLocalMapRespawnedCellIndicator)", game.local_map_reset_cells);
        switch!(app, "hold-release-throws", "Hold and Release Throws", "A grenade or thrown weapon is wound up and held while you hold attack, and thrown when you let go. (Stewie's Tweaks bHoldAndReleaseThrowables)", game.hold_release_throws);
        switch!(app, "npcs-disarm-player", "Enemies Can Disarm You", "A hit that cripples one of your arms knocks the weapon from your hand to the ground, as yours does to them. (Stewie's Tweaks bNPCsCanDisarmPlayer)", game.npcs_disarm_player);
        switch!(app, "npcs-drop-live-grenades", "NPCs Drop Live Grenades", "Someone killed with a grenade in hand drops it live at their feet, where it goes off. (Stewie's Tweaks bNPCsDropLiveGrenades)", game.npcs_drop_live_grenades);
        switch!(app, "water-shows-h2o", "Water Sources Show H2O", "A tap's or puddle's prompt shows how much H2O a drink takes off. (Stewie's Tweaks bWaterSourcesShowH2O)", game.water_shows_h2o);
        switch!(app, "no-respec", "No Character Remake Offer", "Leaving Doc Mitchell's area does not offer to change your character. (Stewie's Tweaks bDisableCharacterRespec)", game.no_respec);
        switch!(app, "weapon-impacts", "Weapon Impact Effects", "A shot that strikes the weapon in someone's hands sparks and sounds like metal, with no blood. (Stewie's Tweaks bWeaponImpactEffects)", game.weapon_impacts);
        switch!(app, "robots-heal-with-scrap", "Robot Companions Heal With Scrap Metal", "The companion wheel's Heal gives a robot companion Scrap Metal instead. (Stewie's Tweaks bRobotCompanionsHealWithScrapMetal)", game.robots_heal_with_scrap);
        amount!(app, "robot-heal-base", "Scrap Metal: Health Each", "Health one Scrap Metal gives a robot companion. (Stewie's Tweaks bRobotCompanionsHealWithScrapMetal, fBase)", 200.0, f32, game.robot_heal_base);
        amount!(app, "robot-heal-per-repair", "Scrap Metal: Health per 10 Repair", "Health added for each 10 points of your Repair skill. (Stewie's Tweaks bRobotCompanionsHealWithScrapMetal, fRepairMult)", 50.0, f32, game.robot_heal_per_repair);
        amount!(app, "robot-heal-expert", "Scrap Metal: Robotics Expert Bonus", "Health added when you have Robotics Expert. (Stewie's Tweaks bRobotCompanionsHealWithScrapMetal, fRoboticsExpertBonus)", 200.0, f32, game.robot_heal_expert);
        switch!(app, "no-weapon-move-penalty", "No Weapon Movement Penalty", "A drawn two-handed weapon does not slow the player's run. (Stewie's Tweaks bNoEquippedWeaponMovementPenalty)", no_weapon_move_penalty);
        switch!(app, "unequip-broken-armor", "Unequip Broken Armour", "The player's armour is taken off when it breaks, staying in the inventory. (Stewie's Tweaks bUnequipBrokenArmor)", unequip_broken_armor);
        switch!(app, "hide-crosshair-first", "No Crosshair in First Person", "The crosshair is not drawn in first person; the activation prompt stays. (Stewie's Tweaks bHideCrosshairInFirstPerson)", hide_crosshair_first);
        switch!(app, "hide-crosshair-third", "No Crosshair in Third Person", "The crosshair is not drawn in third person; the activation prompt stays. (Stewie's Tweaks bHideCrosshairInThirdPerson)", hide_crosshair_third);
        switch!(app, "no-skill-books-at-max", "No Skill Books at 100", "A skill book whose skill is already at 100 is not read from the Pip-Boy, so it is not wasted. (Stewie's Tweaks bNoSkillBooksAbove100)", game.no_skill_books_at_max);
        part!(app, "red-hp-below", "Red Health Bar Below", "The HUD's health bar turns red below this share of health; 0% never. (Stewie's Tweaks bColoredHUDBars)", red_hp_below);
        part!(app, "red-ap-below", "Red Action Point Bar Below", "The HUD's Action Point bar turns red below this share; 0% never. (Stewie's Tweaks bColoredHUDBars)", red_ap_below);
        part!(app, "weapon-cnd-red-below", "Red Weapon Condition Below", "The HUD's weapon condition bar turns red below this condition; 0% never. (Stewie's Tweaks bColorWeaponCndLabel)", weapon_cnd_red_below);
        part!(app, "weapon-cnd-flash-below", "Flashing Weapon Condition Below", "The HUD's weapon condition bar flashes red below this condition; 0% never. (Stewie's Tweaks bColorWeaponCndLabel)", weapon_cnd_flash_below);
        switch!(app, "blackjack-total", "Blackjack Hand Total", "The Blackjack screen shows the total of the hand being played. (Stewie's Tweaks bBlackJackTotalDisplay)", blackjack_total);
        amount!(app, "player-pain-gap", "Less Frequent Pain Sounds", "Seconds after a pain sound before the player makes another; 0 changes nothing. (Stewie's Tweaks bLessFrequentPlayerPainSounds)", 10.0, f64, game.player_pain_gap);
        switch!(app, "pause-on-load", "Pause On Load", "A loaded save starts in the pause menu instead of straight into play. (Stewie's Tweaks bPauseOnSaveLoad)", game.pause_on_load);
        switch!(app, "swap-ammo-holstered", "Swap Ammo While Holstered", "Ammo Swap changes the ammo type with the weapon put away; it reloads when drawn. (Stewie's Tweaks bSwapAmmoWithWeaponHolstered)", game.swap_ammo_holstered);
        switch!(app, "slots-hold-to-spin", "Slots: Hold to Spin", "Holding W at a slot machine spins it again each time the reels stop. (Stewie's Tweaks bSlotsAutoSpinHotkey)", slots_hold_to_spin);
        switch!(app, "no-auto-continue-dialogue", "Click to Continue Dialogue", "In a conversation each line waits for a click before the next, giving time to read. (Stewie's Tweaks bNoAutoContinueDialog)", game.no_auto_continue_dialogue);
        switch!(app, "shift-talks-sneaking", "Shift to Talk While Sneaking", "While sneaking, holding Shift talks to a person instead of picking their pocket. (Stewie's Tweaks bTalkWhileSneakingIfShiftIsHeld)", shift_talks_sneaking);
        switch!(app, "no-quest-set-on-objective", "No Active Quest From New Objectives", "With no quest active, a new objective does not make its quest the active one. (Stewie's Tweaks bDontSetQuestWhenObjectivesAdded)", game.no_quest_set_on_objective);
        switch!(app, "no-quest-set-on-complete", "No Active Quest After Completing One", "When the active quest is done, no other quest becomes active by itself. (Stewie's Tweaks bDontSetQuestWhenCompletingObjectives)", game.no_quest_set_on_complete);
        switch!(app, "hide-read-notes", "Hide Read Notes", "The Pip-Boy's notes list leaves out notes already read. (Stewie's Tweaks bHideReadNotes)", game.hide_read_notes);
        switch!(app, "tab-closes-terminals", "Tab Closes Terminals", "Tab closes a terminal from any page instead of going back a page (Backspace still goes back). (Stewie's Tweaks bTabClosesTerminals)", tab_closes_terminals);
        switch!(app, "tab-back-in-pause", "Tab Goes Back in the Pause Menu", "Tab goes back a page in the pause menu, as Backspace does (the Settings and Controls pages keep Tab for their tabs). (Stewie's Tweaks bTabBackInStartMenu)", tab_back_in_pause);
        switch!(app, "unsafe-fast-travel", "Fast Travel From Anywhere", "Fast travel works from places flagged against it, such as interiors; leaving a scripted place this way can break it. (Stewie's Tweaks bAllowUnsafeFastTravel)", game.unsafe_fast_travel);
        switch!(app, "vats-auto-head", "V.A.T.S. Starts on the Head", "In the classic V.A.T.S., a target's head is selected first rather than the part easiest to hit. (Stewie's Tweaks bVATSAutoTargetHead)", game.vats_auto_head);
        switch!(app, "vats-drops-disarmed-shots", "V.A.T.S. Drops Shots at a Lost Weapon", "Attacks queued in V.A.T.S. on someone's weapon are not played once they no longer hold it. (Stewie's Tweaks bVATSDequeueWeaponShotsOnDisarm)", game.vats_drops_disarmed_shots);
        switch!(app, "vats-target-projectiles", "Target Projectiles in V.A.T.S.", "V.A.T.S. can mark projectiles in flight: explosives go off, others end; odds: distance, skill. (Stewie's Tweaks bTargetProjectilesInVATS)", game.vats_target_projectiles);
        switch!(app, "vats-target-mines", "Target Projectiles: Mines Too", "With Target Projectiles in V.A.T.S., mines can be marked too, thrown or lying armed. (Stewie's Tweaks bTargetProjectilesInVATS, bMines)", game.vats_target_mines);
        switch!(app, "vats-target-own-projectiles", "V.A.T.S. Targets Your Own Projectiles", "With Target Projectiles in V.A.T.S., your own projectiles can be marked too. (Stewie's Tweaks bVATSTargetPlayerProjectiles)", game.vats_target_own_projectiles);
        switch!(app, "vats-ignore-non-explosive", "V.A.T.S. Ignores Non-Explosive Projectiles", "With Target Projectiles in V.A.T.S., only projectiles with an explosion can be marked. (Stewie's Tweaks bVATSIgnoreNonExplosiveProjectiles)", game.vats_ignore_non_explosive);
        switch!(app, "vats-burst-stops-on-kill", "V.A.T.S. Burst Stops on a Kill", "The rest of a burst in V.A.T.S. is not fired once its target is dead. (Stewie's Tweaks bVATSStopBurstIfTargetDead)", game.vats_burst_stops_on_kill);
        switch!(app, "repair-needs-weapon-skill", "Repair Needs the Weapon's Skill", "The player can repair a weapon only with Repair at least the weapon's skill requirement. (Stewie's Tweaks bRepairRequiresWeaponSkill)", game.repair_needs_weapon_skill);
        add_choice_list(
            app,
            Setting {
                key: "tweak-date-style",
                group: GROUP,
                label: "Date Format",
                note: "How the Pip-Boy and the wait menu write the date. (Stewie's Tweaks iDateFormat)",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            ChoiceList {
                names: |_| choice_names(&crate::wait::model::DATE_STYLES),
                chosen: |w| crate::wait::model::DATE_STYLES[w.get_resource::<TweakSettings>().map_or(0, |t| t.date_style).min(crate::wait::model::DATE_STYLES.len() - 1)].to_string(),
                pick: take_date_style,
                save: |w| crate::wait::model::DATE_STYLES[w.get_resource::<TweakSettings>().map_or(0, |t| t.date_style).min(crate::wait::model::DATE_STYLES.len() - 1)].to_string(),
                load: take_date_style,
                ask_keep: false,
            },
        );
        switch!(app, "hide-unknown-names", "Hide Unknown Names", "The activation prompt gives no name for someone who has not spoken to the player yet (bodies are still named). (Stewie's Tweaks bHideUnknownNPCNames)", game.hide_unknown_names);
        switch!(app, "vats-keeps-holstered", "V.A.T.S. Puts the Weapon Back", "A weapon that V.A.T.S. drew is put away again when the mode ends with nothing fired. (Stewie's Tweaks bEnteringVATSDoesntUnholsterWeapon)", vats_keeps_holstered);
        switch!(app, "no-hotkey-wheel", "No Hotkey Wheel", "Holding a hotkey does not open the weapon wheel; letting go uses the slot as a tap does. (Stewie's Tweaks bNoHUDHotkeyPopup)", no_hotkey_wheel);
        switch!(app, "ammo-label-under", "Ammo Label Under the Count", "The HUD writes the ammo type under the ammo count instead of beside it. (Stewie's Tweaks bMoveAmmoTypeLabel)", ammo_label_under);
        add_choice_list(
            app,
            Setting {
                key: "tweak-companion-pip",
                group: GROUP,
                label: "Companion Compass Colour",
                note: "The colour of companions' marks on the compass; Off leaves them as anyone's. (Stewie's Tweaks bCompanionPipColorChange)",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            ChoiceList {
                names: |_| COMPANION_COLOURS.iter().map(|(n, _)| (*n).to_string()).collect(),
                chosen: |w| COMPANION_COLOURS[w.get_resource::<TweakSettings>().map_or(0, |t| t.companion_pip).min(COMPANION_COLOURS.len() - 1)].0.to_string(),
                pick: take_companion_pip,
                save: |w| COMPANION_COLOURS[w.get_resource::<TweakSettings>().map_or(0, |t| t.companion_pip).min(COMPANION_COLOURS.len() - 1)].0.to_string(),
                load: take_companion_pip,
                ask_keep: false,
            },
        );
        switch!(app, "female-facial-hair", "Facial Hair for Women", "The character creator and the barber offer facial hair to a woman too. (Stewie's Tweaks bRaceMenuAllowFemaleFacialHair)", female_facial_hair);
        switch!(app, "terminal-password-prompt", "Show Use Password", "A locked terminal whose password note the player holds says so in its prompt until first opened. (Stewie's Tweaks bShowUsePasswordOnTerminalsWithNote)", game.terminal_password_prompt);
        switch!(app, "lockpick-move-keys", "Lockpicking With the Movement Keys", "Lockpicking: strafe moves the pin (Shift fast, Ctrl slow), Forward, Back, Jump the driver. (Stewie's Tweaks bLockPickMenuKeyboardMovement)", game.lockpick_move_keys);
        switch!(app, "store-all", "Store All", "In a container, Shift with Take All stores everything listed but what is worn or a quest's. (Stewie's Tweaks bContainerMenuStoreAllHotkey)", store_all);
        switch!(app, "map-recentre-key", "Map Recentre Key", "C puts the Pip-Boy's local or world map back on the player after it was dragged. (Stewie's Tweaks bMapRecenterHotkey)", game.map_recentre_key);
        switch!(app, "map-marker-distance", "Map Marker Distance", "The World Map says how far the marker under the pointer is from the player. (Stewie's Tweaks bMapLocationDisplayDistance)", game.map_marker_distance);
        switch!(app, "quantity-shows-price", "Quantity Box Shows the Price", "In a barter, the \"How many?\" box says what the amount costs. (Stewie's Tweaks bBarterQuantityMenuShowsPrice)", game.quantity_shows_price);
        switch!(app, "quantity-shows-weight", "Quantity Box Shows the Weight", "In a container, the \"How many?\" box says what the amount weighs. (Stewie's Tweaks bContainerQuantityMenuShowsWeight)", game.quantity_shows_weight);
        switch!(app, "hide-empty-reputation", "Hide an Empty Reputation List", "With no reputations yet, the Stats page's General tab shows the statistics only, with no switch to the reputations. (Stewie's Tweaks bHideReputationTabIfEmpty)", game.hide_empty_reputation);
        switch!(app, "quick-load-in-menus", "Quick Load From the Menus", "The QuickLoad key loads the quicksave from the main and pause menus too. (Stewie's Tweaks bStartMenuQuickLoad)", quick_load_in_menus);
        switch!(app, "preview-perks", "Preview Perks on Level Up", "A level-up without a perk still shows the perks page, to look at with none to take. (Stewie's Tweaks bPreviewPerksOnLevelUp)", game.preview_perks);
        switch!(app, "pick-locks-with-key", "Pick Locks Even With the Key", "Holding Shift, a lock the player has the key of opens the lockpicking game instead. (Stewie's Tweaks bPickLocksEvenWithKey)", pick_locks_with_key);
        switch!(app, "right-click-previous-category", "Right Click for the Previous Category", "In a container or a barter, a right click on a list's heading goes back a category. (Stewie's Tweaks bRightClickChangesToPreviousContainerCategory)", right_click_previous_category);
        switch!(app, "mark-notes-unread", "Mark Notes Unread", "A right click on a note in the Pip-Boy marks it unread again. (Stewie's Tweaks bMarkNotesUnread)", game.mark_notes_unread);
        switch!(app, "full-barter-total", "Barter Total Past the Merchant's Caps", "Barter shows the whole total past the merchant's caps, though it pays only those. (Stewie's Tweaks bShowBarterTotalWhenOverSellLimit)", full_barter_total);
        switch!(app, "compass-fade-left", "Compass Fades on the Left", "Marks on the compass fade toward its left end (quest marks stay whole), for a compass seen from its middle. (Stewie's Tweaks bCompassFadeLeftSide)", compass_fade_left);
        switch!(app, "custom-dead-zones", "Controller Dead Zones", "The sticks use the dead zones below instead of the game's (12% of a push); 0% takes every movement. (Stewie's Tweaks bDisableControllerDeadzones)", custom_dead_zones);
        part!(app, "left-dead-zone", "Left Stick Dead Zone", "The share of a push the left stick ignores, with Controller Dead Zones on. (Stewie's Tweaks iControllerDeadzone)", left_dead_zone);
        part!(app, "right-dead-zone", "Right Stick Dead Zone", "The share of a push the right stick ignores, with Controller Dead Zones on. (Stewie's Tweaks iControllerDeadzoneRS)", right_dead_zone);
        switch!(app, "custom-trigger-dead-zones", "Trigger Dead Zones", "LT and RT count as pressed at the points below instead of three quarters down. (Stewie's Tweaks bControllerTriggerDeadzones)", custom_trigger_dead_zones);
        part!(app, "lt-dead-zone", "Left Trigger Press Point", "How far LT goes down before it counts as pressed, with Trigger Dead Zones on. (Stewie's Tweaks iDeadzoneLT)", lt_dead_zone);
        part!(app, "rt-dead-zone", "Right Trigger Press Point", "How far RT goes down before it counts as pressed, with Trigger Dead Zones on. (Stewie's Tweaks iDeadzoneRT)", rt_dead_zone);
        switch!(app, "responsive-stick", "More Responsive Controller Aiming", "Right-stick turning grows in a line past the dead zone, not squared: small pushes count. (Stewie's Tweaks bMoreResponsiveControllerAiming)", responsive_stick);
        switch!(app, "faster-pad-orbit", "Faster Controller Camera Circling", "With a controller, the camera circles the player twice as fast while Toggle POV is held. (Stewie's Tweaks bFasterControllerPOVRotate)", faster_pad_orbit);
        switch!(app, "vanity-returns-first", "Vanity Camera Returns to First Person", "A first-person view comes back after the circling camera (idle, Toggle POV held). (Stewie's Tweaks bVanityCamReturnsToFirstPerson)", vanity_returns_first);
        add_setting(
            app,
            Setting::slider(
                "tweak-horizontal-look",
                GROUP,
                "Horizontal Look Speed",
                "Left and right look speed apart from up and down, on top of the sensitivity; x1.0 changes nothing. (Stewie's Tweaks bSeparateHorizontalSensitivity)",
                Slider {
                    get: |w| part(w.get_resource::<TweakSettings>().map_or(1.0, |t| t.horizontal_look)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.horizontal_look = factor(v);
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(shown_factor),
                },
            ),
        );
        switch!(app, "vats-shift-ignores-friends", "Shift Ignores Friends in V.A.T.S.", "Holding Shift, V.A.T.S. marks no one who would not fight the player. (Stewie's Tweaks bShiftIgnoresFriendlyVATS)", game.vats_shift_ignores_friends);
        switch!(app, "remember-console-history", "Remember Console History", "The console's entered lines are kept between runs, in ConsoleHistory.txt in NVForever's folder. (Stewie's Tweaks bRememberConsoleHistory)", remember_console_history);
        switch!(app, "console-output-file", "Console Output File", "Console lines are added to ConsoleOutput.txt in NVForever's folder from the start. (Stewie's Tweaks bUseConsoleOutputFile)", console_output_file);
        switch!(app, "console-text-shadow", "Console Text Shadow", "The console's lines are a darker green over a faint shadow, easier to read on a bright view. (Stewie's Tweaks bConsoleTextShadow)", console_text_shadow);
        switch!(app, "click-to-exit-loading", "Press to Leave Loading Screens", "A loading screen stays up once everything is in until a key or a button is pressed. (Stewie's Tweaks bClickToExitLoadScreens)", click_to_exit_loading);
        switch!(app, "select-unavailable-radios", "Keep Stations Out of Range", "Received stations stay on the Pip-Boy's list out of range; tuned to one, it plays once received again. (Stewie's Tweaks bSelectUnavailableRadios)", game.select_unavailable_radios);
        switch!(app, "radiation-radio-static", "Radiation Brings Radio Static", "Taking radiation brings static into the Pip-Boy's radio, more the faster the rads come. (Stewie's Tweaks bRadiationCausesRadioStatic)", game.radiation_radio_static);
        switch!(app, "barter-caps-change", "Barter Shows Caps After", "In a barter, each side's caps after the trade are shown beside its caps now. (Stewie's Tweaks bBarterShowCapsChange)", barter_caps_change);
        switch!(app, "no-self-meltdown", "No Self Meltdown Damage", "Blasts given off by the Meltdown perk hurt neither the player nor the companions. (Stewie's Tweaks bNoSelfMeltdownDamage)", game.no_self_meltdown);
        add_setting(
            app,
            Setting::slider(
                "tweak-max-level",
                GROUP,
                "Level Cap",
                "The highest level the player can reach, with no levels added for the add-ons; 0 keeps the game's. (Stewie's Tweaks iMaxCharacterLevel)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| t.game.max_level as f32 / MAX_LEVEL_CAP),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.max_level = (v * MAX_LEVEL_CAP).round() as u32;
                        }
                    },
                    min: 0.0,
                    step: 1.0 / MAX_LEVEL_CAP,
                    shown: Some(|v| match (v * MAX_LEVEL_CAP).round() as u32 {
                        0 => "the game's".to_string(),
                        n => n.to_string(),
                    }),
                },
            ),
        );
        switch!(app, "standing-sneak-attacks", "Sneak Attacks Standing", "An attack on someone who has not detected the player is a sneak attack without crouching too. (Stewie's Tweaks bSneakAttackWithoutCrouching)", game.standing_sneak_attacks);
        switch!(app, "no-companion-item-damage", "No Companion Item Damage", "The companions' weapons do not wear with use. (Stewie's Tweaks bNoCompanionItemDamage)", game.no_companion_item_damage);
        switch!(app, "modding-needs-skill", "Weapon Mods Need the Skill", "A weapon can be modded only by a player who meets its skill requirement. (Stewie's Tweaks bWeaponModdingSkillRequirement)", game.modding_needs_skill);
        switch!(app, "planting-needs-skill", "Planting Grenades Needs the Skill", "A grenade or mine placed in someone's pocket goes live only if the player meets its skill requirement. (Stewie's Tweaks bPlantingLiveGrenadesRequiresWeaponSkill)", game.planting_needs_skill);
        switch!(app, "travel-costs-bottle", "Fast Travel Costs a Nuka-Cola", "Each fast travel takes a Nuka-Cola Quantum, Quartz or Victory, and is refused without one. (Stewie's Tweaks bFastTravelCostsSpecialNukaBottles)", game.travel_costs_bottle);
        switch!(app, "jump-swims-up", "Jump Swims Upwards", "Holding Jump under water swims up, at the walking stroke's speed. (Stewie's Tweaks bJumpSwimsUpwards)", jump_swims_up);
        switch!(app, "armor-sinks", "Armour Causes Sinking", "Medium and heavy body armour pull the player down in water (heavy faster). (Stewie's Tweaks bArmorCausesSinking)", game.armor_sinks);
        switch!(app, "remember-pin-health", "Remember Bobby Pin Wear", "The bobby pin keeps its wear from one lock to the next, kept in the save. (Stewie's Tweaks bRememberBobbyPinHealth)", game.remember_pin_health);
        switch!(app, "force-spares-lock", "Forcing Breaks the Pin, Not the Lock", "A failed Force Lock breaks the bobby pin in it and the lock can be tried again. (Stewie's Tweaks bForceLockpickNoBreakLock)", game.force_spares_lock);
        switch!(app, "no-condition-damage-penalty", "No Condition Damage Penalty", "Weapons deal their full damage whatever their condition. (Stewie's Tweaks bNoWeaponConditionDamagePenalty)", game.no_condition_damage_penalty);
        switch!(app, "no-condition-damage-buffer", "No Condition Damage Buffer", "A weapon's damage falls as soon as its condition is below full, not only below 75%. (Stewie's Tweaks bRemoveWeaponDamageBuffer)", game.no_condition_damage_buffer);
        switch!(app, "crit-ignores-fire-rate", "Automatic Criticals Not Cut by Fire Rate", "Automatic weapons' critical chance is not divided by their rate of fire. (Stewie's Tweaks bCritChanceIgnoresFireRate)", game.crit_ignores_fire_rate);
        add_setting(
            app,
            Setting::slider(
                "tweak-crit-damage-scale",
                GROUP,
                "Critical Damage",
                "Every critical hit's added damage is multiplied by this; x1.0 changes nothing. (Stewie's Tweaks bScaleCriticalDamage)",
                Slider {
                    get: |w| part(w.get_resource::<TweakSettings>().map_or(1.0, |t| t.game.crit_damage_scale)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.crit_damage_scale = factor(v);
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(shown_factor),
                },
            ),
        );
        switch!(app, "npc-dr-ignores-condition", "NPC Armour DR Ignores Condition", "An NPC's armour keeps its whole Damage Resistance however worn. (Stewie's Tweaks bNoScaleNpcDamageResistanceByCondition)", game.npc_dr_ignores_condition);
        switch!(app, "npc-dt-ignores-condition", "NPC Armour DT Ignores Condition", "An NPC's armour keeps its whole Damage Threshold however worn. (Stewie's Tweaks bNoScaleNpcDamageThresholdByCondition)", game.npc_dt_ignores_condition);
        switch!(app, "explosion-radius-buff", "Explosion Radius Buff", "The inner 30% of an explosion's radius deals full damage, falling to 20% at the edge. (Stewie's Tweaks bExplosionRadiusBuff)", game.explosion_radius_buff);
        switch!(app, "npcs-sneak-crit-player", "NPCs Sneak Attack the Player", "An NPC the player has not detected lands sneak attack criticals on them, as the player does. (Stewie's Tweaks bNPCsCanSneakCritPlayer)", game.npcs_sneak_crit_player);
        switch!(app, "fatal-crits-gib", "Fatal Criticals Always Gib", "A killing critical hit that is no sneak attack always blows off or severs the part struck. (Stewie's Tweaks bFatalNonSneakCritsAlwaysGib)", game.fatal_crits_gib);
        switch!(app, "fall-deaths-dismember", "Deadly Falls Take the Legs", "An actor killed by a fall loses its legs, blown off where they can be. (Stewie's Tweaks bFallDeathsCauseDismemberment)", game.fall_deaths_dismember);
        switch!(app, "splash-torso-gib", "Splash Kills Can Gib the Torso", "A blast kill by someone with Bloody Mess has a one-in-four chance to blow the torso apart. (Stewie's Tweaks bSplashDamageTorsoGibbing)", game.splash_torso_gib);
        switch!(app, "npcs-drop-holstered", "NPCs Drop Holstered Weapons", "An NPC that dies with its weapon put away drops it, as one with it drawn does. (Stewie's Tweaks bNPCsDropWeaponHolsteredWeapon)", game.npcs_drop_holstered);
        switch!(app, "npcs-earn-casings", "NPCs Keep Spent Cases", "NPCs keep the cases, hulls and drained cells their shots leave, as the player does, to be found on them. (Stewie's Tweaks bNPCsEarnAmmoCasings)", game.npcs_earn_casings);
        switch!(app, "first-person-vats", "First Person V.A.T.S.", "V.A.T.S.'s shots play in first person; the view comes back after. (Stewie's Tweaks bFirstPersonVATS)", first_person_vats);
        switch!(app, "no-ammo-casings", "No Spent Cases", "The player's shots leave no cases, hulls or drained cells in the inventory. (Stewie's Tweaks bNoAmmoCasings)", game.no_ammo_casings);
        switch!(app, "crosshair-when-aiming", "Crosshair Down the Sights", "The crosshair stays on screen while aiming down iron sights; a scope still hides it. (Stewie's Tweaks bKeepCrosshairWhenAiming)", crosshair_when_aiming);
        switch!(app, "move-during-vats-camera", "Move During V.A.T.S. Camera", "You can still walk and run while V.A.T.S.'s camera plays the shots; jumping, firing and aiming stay held. (Stewie's Tweaks bMoveDuringVATSPlayback)", move_during_kill_cam);
        switch!(app, "stack-figures", "Whole-Stack Pickup Figures", "Looking at a stack of items, the weight and value shown are the whole stack's, not one item's. (Stewie's Tweaks bBetterPickupPrompt)", stack_figures);
        switch!(app, "seated-wide", "Turn Further While Seated", "Seated in first person, you can look 115 degrees to either side instead of 80. (Stewie's Tweaks bTurnFurtherWhileSeated)", seated_wide);
        switch!(app, "no-turn-clip", "No Turning Animation", "Your character's body does not step round when you turn on the spot, so it does not slide on after you stop. (Stewie's Tweaks bNoTurningAnim)", no_turn_clip);
        switch!(app, "no-land-clip", "No Landing Animation", "Your character does not play a landing movement after a jump or a fall. (Stewie's Tweaks bRemoveLandingAnim)", no_land_clip);
        switch!(app, "hide-unmapped-places", "Compass: Only Mapped Places", "A place you have not found yet shows on the compass only if it is already marked on your map. (Stewie's Tweaks bHideInvisibleUndiscoveredLocations)", hide_unmapped_places);
        switch!(app, "vigor-review-first", "S.P.E.C.I.A.L. Machine: Start on Summary", "The machine that sets your S.P.E.C.I.A.L. opens on its last page, all seven at once, instead of one page each. (Stewie's Tweaks bSkipVigorTesterSpecialPages)", vigor_review_first);
        amount!(app, "scope-delay", "Scope Delay (ms)", "Milliseconds (to 1000) a scope waits once zoomed before covering the screen; 0 shows it at once. (Stewie's Tweaks bScopeVisibilityDelay)", 1000.0, f32, scope_delay_ms);
        switch!(app, "no-pointer-message-box", "No Pointer Over Message Boxes", "The mouse pointer is hidden while a message box is up; choose with the keys or the controller. (Stewie's Tweaks bHideCursorInMessageMenu)", no_pointer_in_message_box);
        switch!(app, "no-pointer-dialogue", "No Pointer in Conversations", "The mouse pointer is hidden over a conversation's choices; choose with keys, the wheel or the controller. (Stewie's Tweaks bHideCursorInDialog)", no_pointer_in_dialogue);
        switch!(app, "levelup-wheel", "Level Up: Wheel Sets Skills", "On level-up screens the mouse wheel raises or lowers the stat under the pointer. (Stewie's Tweaks bLevelUpScrollWheelSupport)", levelup_wheel);
        switch!(app, "compare-armor", "Compare Armor", "A piece of armour's DT in the Pip-Boy shows the difference from what you wear in the same places. (Stewie's Tweaks bCompareArmorStats)", game.compare_armor);
        switch!(app, "detailed-stats", "Detailed Stats", "In the Pip-Boy's stats, a raised or lowered value says by how much, as (+5) for (+), and values show past 10 and 100. (Stewie's Tweaks bMoreDetailedStatsMenu)", game.detailed_stats);
        switch!(app, "minigames-timescale", "Lockpicking Follows the Time Speed", "When a script speeds up or slows down time, the lock turns and the pick wears at that speed too. (Stewie's Tweaks bTimescaleAffectsMinigames)", game.minigames_timescale);
        switch!(app, "no-name-limit", "No Name Length Limit", "Your character's name may be longer than 24 letters. (Stewie's Tweaks bNoPlayerNameLimit)", game.no_name_limit);
        switch!(app, "seat-keeps-on-activate", "Activate Doesn't Stand Up", "Seated or lying down, the activate key no longer gets you up; a movement key does. (Stewie's Tweaks bActivatingDoesntStandUp)", seat_keeps_on_activate);
        switch!(app, "plain-going-out", "Plain Screen Going Outside", "Going outdoors loads behind a plain black screen, no picture or tip; loading a save is unchanged. (Stewie's Tweaks bNoExteriorLoadScreens)", plain_going_out);
        switch!(app, "mark-recurring-challenges", "Mark Repeating Challenges", "A challenge that starts again each time it is done says \"(repeats)\" beside its progress in the Pip-Boy. (Stewie's Tweaks bRecurringChallengeIndicator)", game.mark_recurring_challenges);
        switch!(app, "hide-hotkeyed-containers", "Hide Hotkeyed Items in Containers", "Things on a hotkey are not listed on your side of a container. (Stewie's Tweaks bHideHotkeyedItemsInContainers)", hide_hotkeyed_containers);
        switch!(app, "hide-hotkeyed-barter", "Hide Hotkeyed Items in Barter", "Things on a hotkey are not offered when trading. (Stewie's Tweaks bHideHotkeyedItemsInBarter)", hide_hotkeyed_barter);
        switch!(app, "hide-hotkeyed-repair", "Hide Hotkeyed Items in Repair", "Things on a hotkey are not offered as parts when repairing in the Pip-Boy. (Stewie's Tweaks bHideHotkeyedItemsInRepairLists)", hide_hotkeyed_repair);
        switch!(app, "map-zoom-at-pointer", "Map Zooms at the Pointer", "The mouse wheel zooms the Pip-Boy's maps in and out about the place under the pointer, not the middle. (Stewie's Tweaks bBetterMapZoom)", game.map_zoom_at_pointer);
        switch!(app, "console-homes", "Console Homes", "Console: sethome, home and delhome [name]; back undoes the last home or back. Kept until you quit. (Stewie's Tweaks bAddSetHomeCommand)", console_homes);
        switch!(app, "shift-screenshot-hides", "Shift Screenshot Hides the HUD", "Holding Shift as you take a screenshot leaves the HUD, crosshair and messages out of the picture. (Stewie's Tweaks bShiftScreenshotHidesMenus)", shift_screenshot_hides);
        switch!(app, "print-plugin-names", "Console Lines Name Their Plugin", "A line a script prints to the console starts with the name of the plugin it comes from, in brackets. (Stewie's Tweaks bModConsolePrintsIncludeName)", game.print_plugin_names);
        switch!(app, "weapon-requirements-matter", "Weapon Requirements Matter", "A weapon cannot be equipped while your Strength or its skill is below what it asks. (Stewie's Tweaks bWeaponRequirementsMatter)", game.weapon_requirements_matter);
        switch!(app, "console-model-details", "Console Shows Models", "Selecting something in the console also prints the path of its model; its cell's Editor ID and name are always shown. (Stewie's Tweaks bExtraConsoleDetails)", console_model_details);
        part!(app, "back-pace", "Backward Pace", "How fast you move backwards, as a part of your usual pace; 100% changes nothing. (Stewie's Tweaks bMovementPenalties)", back_pace);
        part!(app, "strafe-pace", "Sideways Pace", "How fast you move sideways, as a part of your usual pace; 100% changes nothing. Diagonals fall between. (Stewie's Tweaks bMovementPenalties)", strafe_pace);
        switch!(app, "hack-no-single-chars", "Hacking: Symbols Do Nothing", "Clicking a lone symbol while hacking does nothing and prints nothing. (Stewie's Tweaks bImprovedHacking)", game.hack_no_single_chars);
        switch!(app, "hack-no-repeat-words", "Hacking: No Repeat Guesses", "A word you have already tried cannot be tried again, so it costs no attempt. (Stewie's Tweaks bImprovedHacking)", game.hack_no_repeat_words);
        switch!(app, "hack-dud-when-full", "Hacking: Dud When Attempts Full", "A bracket pair found with all attempts left removes a dud instead of giving attempts back. (Stewie's Tweaks bImprovedHacking)", game.hack_dud_when_full);
        switch!(app, "hack-spare-guessed", "Hacking: Duds Spare Tried Words", "Removing a dud leaves the words you have already tried, unless only they are left. (Stewie's Tweaks bImprovedHacking)", game.hack_spare_guessed);
        switch!(app, "hack-mark-guesses-as-duds", "Hacking: Mark Guesses as Duds", "Wrong guesses turn into dots in the dump like removed duds. (Stewie's Tweaks bImprovedHacking, bMarkGuessesAsDuds)", game.hack_mark_guesses_as_duds);
        switch!(app, "hack-keep-matching-chars", "Hacking: Keep Matching Characters", "With Mark Guesses as Duds, letters matching the password stay visible in place. (Stewie's Tweaks bMarkGuessesAsDudsKeepMatchingCharacters)", game.hack_keep_matching_chars);
        switch!(app, "hack-compact-guesses", "Hacking: Compact Guess Log", "Prints each wrong guess on one log line with its likeness instead of three lines. (Stewie's Tweaks bImprovedHacking, bCompactGuesses)", game.hack_compact_guesses);
        add_setting(
            app,
            Setting::slider(
                "tweak-hack-replenish-bonus",
                GROUP,
                "Hacking: Replenish Bonus Attempts",
                "Extra attempts a Replenished bracket adds instead of resetting to 4; 0 resets to 4. (Stewie's Tweaks iAllowanceReplenishedBonus)",
                Slider {
                    get: |w| w.get_resource::<TweakSettings>().map_or(0.0, |t| (f32::from(t.game.hack_replenish_bonus) / 10.0).clamp(0.0, 1.0)),
                    set: |w, v| {
                        if let Some(mut t) = w.get_resource_mut::<TweakSettings>() {
                            t.game.hack_replenish_bonus = (v * 10.0).round() as u8;
                        }
                    },
                    min: 0.0,
                    step: 0.1,
                    shown: Some(|v| format!("{:.0}", (v * 10.0).round())),
                },
            ),
        );
        switch!(app, "run-overencumbered", "Run While Overencumbered", "Carrying too much, you can still run and jump by spending Action Points; with none left you walk. (Stewie's Tweaks bOverencumberedTweak)", run_overencumbered);
        amount!(app, "over-run-ap", "Overencumbered Running AP", "Action Points running costs each second while carrying too much (with Run While Overencumbered). (Stewie's Tweaks bOverencumberedTweak)", 20.0, f32, over_run_ap);
        amount!(app, "over-jump-ap", "Overencumbered Jump AP", "Action Points a jump costs while carrying too much (with Run While Overencumbered). (Stewie's Tweaks bOverencumberedTweak)", 50.0, f32, over_jump_ap);
        switch!(app, "npc-melee-part-damage", "Enemy Blows Hit Where They Land", "A melee or unarmed blow on you is multiplied by the body part it lands on (more on the head), as your own blows are. (Stewie's Tweaks bLocationalMeleeDamage)", game.npc_melee_part_damage);
        switch!(app, "disarm-needs-skill", "Disarming Needs Skill", "Mine disarming fails more the lower your Explosives and Perception, and may set it off (less with Luck). (Stewie's Tweaks bDisarmRequiresSkill)", game.disarm_needs_skill);
        amount!(app, "disarm-mult", "Disarm Chance Multiplier", "Multiplies the chance to disarm a mine (Disarming Needs Skill); 1 as the formula gives it. (Stewie's Tweaks bDisarmRequiresSkill)", 3.0, f64, game.disarm_mult);
        amount!(app, "disarm-explode-mult", "Set-Off Chance Multiplier", "Multiplies the chance that a failed disarm sets the mine off (Disarming Needs Skill); 0 never. (Stewie's Tweaks bDisarmRequiresSkill)", 3.0, f64, game.disarm_explode_mult);
        switch!(app, "cap-skills-by-special", "Skills Capped by S.P.E.C.I.A.L.", "On leveling up a skill rises only to its base plus its attribute's share (and Luck's), up to the cap below. (Stewie's Tweaks bCapSkillsBySPECIAL)", game.cap_skills_by_special);
        amount!(app, "skill-cap-base", "Skill Cap Base", "The part of every skill's cap before attributes (Skills Capped by S.P.E.C.I.A.L.). (Stewie's Tweaks bCapSkillsBySPECIAL)", 100.0, f64, game.skill_cap_base);
        amount!(app, "skill-cap-per-special", "Skill Cap per Attribute Point", "What each point of the governing attribute adds to a skill's cap. (Stewie's Tweaks bCapSkillsBySPECIAL)", 10.0, f64, game.skill_cap_per_special);
        amount!(app, "skill-cap-per-luck", "Skill Cap per Luck Point", "What each point of Luck adds to every skill's cap. (Stewie's Tweaks bCapSkillsBySPECIAL)", 10.0, f64, game.skill_cap_per_luck);
        amount!(app, "skill-cap-most", "Skill Cap Most", "No skill's cap goes above this (Skills Capped by S.P.E.C.I.A.L.). (Stewie's Tweaks bCapSkillsBySPECIAL)", 100.0, i32, game.skill_cap_most);
        switch!(app, "cap-skills-spare-tagged", "Skill Cap Spares Tagged Skills", "Tagged skills are not capped by Skills Capped by S.P.E.C.I.A.L. (Stewie's Tweaks bCapSkillsBySPECIAL)", game.cap_skills_spare_tagged);
        switch!(app, "hold-breath", "Hold Breath Through a Scope", "Holding Shift while looking through a scope steadies it, costing Action Points while held; with none left the breath goes. (Stewie's Tweaks bScopeHoldBreath)", hold_breath);
        switch!(app, "breath-activate", "Held Breath: Activate Key", "Holding Activate also holds breath through a scope. (Stewie's Tweaks bActivateHoldsBreath)", breath_activate);
        part!(app, "breath-sway", "Held Breath Sway", "How much a scope still sways while the breath is held, as a part of its usual sway. (Stewie's Tweaks bScopeHoldBreath)", breath_sway);
        amount!(app, "breath-ap", "Held Breath AP", "Action Points holding the breath costs each second (Hold Breath Through a Scope). (Stewie's Tweaks bScopeHoldBreath)", 20.0, f32, breath_ap);
        switch!(app, "scope-wheel-zoom", "Wheel Zooms Scopes", "The mouse wheel zooms a scope in (up) and out, half to double the usual, reset each time. (Stewie's Tweaks bAdjustableScopeZoom)", scope_wheel_zoom);
        switch!(app, "wheel-cycles-hotkeys", "Mouse Wheel Changes Weapon", "In first person, rolling the mouse wheel takes out the next (up) or previous weapon on your hotkeys. (Stewie's Tweaks bMousewheelScrollsWeaponHotkeys)", wheel_cycles_hotkeys);
        switch!(app, "dpad-cycles-hotkeys", "D-Pad Changes Weapon", "D-pad right takes out the next hotkeyed weapon, left the one before, not Ammo Swap and hotkey 4. (Stewie's Tweaks bWeaponCycleUpDownHotkeys)", dpad_cycles_hotkeys);
        switch!(app, "keys-with-pad", "Keyboard and Mouse With a Controller", "With a controller, keyboard and mouse still work outside menus. (Stewie's Tweaks bAllowKeyboardAndMouseWithControllerConnected)", keys_with_pad);
        switch!(app, "double-tap-ammo", "Double-Tap Reload Changes Ammo", "Tapping Reload twice quickly swaps the ammo type; one tap reloads after a third of a second. (Stewie's Tweaks bDoubleTapReloadToChangeAmmoType)", double_tap_ammo);
        switch!(app, "improved-weather", "Improved Weather", "A short fast travel keeps the weather instead of rolling a new one for the trip. (Stewie's Tweaks bImprovedWeather)", improved_weather);
        switch!(app, "vats-stop-following-dead", "V.A.T.S. Stops Following the Dead", "A kill camera stops turning after its target once it is dead, holding where it last saw it alive. (Stewie's Tweaks bVATSStopFollowingDeadTargets)", vats_stop_following_dead);
        switch!(app, "night-vision-toggle", "Night Vision Toggle", "With a night-vision scope up, the light's key (holding Tab, or its own key) toggles its night vision. (Stewie's Tweaks bAddNightVisionToggle)", night_vision_toggle);
        switch!(app, "night-vision-off-first", "Night Vision Toggle: Off Until Switched On", "A night-vision scope gives no night vision until you switch it on. (bDisableVisionByDefault)", night_vision_off_first);
        switch!(app, "night-vision-by-day", "Night Vision Toggle: By Day Too", "A night-vision scope gives its night vision by day as well. (bAllowNightVisionDuringDay)", night_vision_by_day);
        switch!(app, "kill-cam-outside-vats", "Kill Camera Outside V.A.T.S.", "Kills outside V.A.T.S. get kill cameras too (Kill camera rules), 20 s apart. (Ours; see Stewie's Tweaks bUseHitLocationInCinematicKillcams)", kill_cam_outside_vats);
        switch!(app, "kill-cam-hit-location", "Kill Camera Focuses on the Hit Location", "A kill camera looks at the part your shot struck: the head, the chest, the legs. (Stewie's Tweaks bUseHitLocationInCinematicKillcams)", kill_cam_hit_location);
        amount!(app, "weather-trip-distance", "Improved Weather: Short Trip", "How far a fast travel may go and keep the weather (about 4,100 units to a cell). (Stewie's Tweaks fFastTravelWeatherChangeDistanceThreshold)", 100000.0, f32, weather_trip_distance);
        switch!(app, "compass-pips-fade", "Fade Pips by Distance", "Compass marks fade with distance, over the range people and creatures could notice you. (Stewie's Tweaks bCompassNPCDistanceBasedAlpha)", compass_pips_fade);
        switch!(app, "compass-places-fade", "Fade Places by Distance", "Places on the compass grow fainter the further away they are, over the distance they show at. (Stewie's Tweaks bCompassLocationDistanceBasedAlpha)", compass_places_fade);
        switch!(app, "buffed-health-bars", "Health Bars Count Buffs", "Target health bars count Health raised by chems and effects: what a buffed enemy can take. (Stewie's Tweaks bEnemyHealthbarShowBuffedHP)", game.buffed_health_bars);
        amount!(app, "recent-dead-secs", "Recently Dead on the Compass (s)", "For how many seconds, up to 120, someone killed nearby stays on the compass, until you look at them; 0 none. (Stewie's Tweaks bRecentlyDeadNPCIndicator)", 120.0, f32, recent_dead_secs);
        switch!(app, "armor-sounds-3d", "Armour Rustle From the Body", "In third person your armour and clothing are heard from your character, not at the camera. (Stewie's Tweaks bArmorSoundsPlayIn3D)", armor_sounds_3d);
        switch!(app, "mine-blasts-heard", "Mine Blasts Are Heard", "A mine going off alerts the people near who would fight whoever laid it, or whoever set it off. (Stewie's Tweaks bNPCsDetectMineExplosions)", game.mine_blasts_heard);
        switch!(app, "blasts-spare-items", "Explosions Leave Items Be", "Explosions do not throw about things you can pick up, so loot stays where it lies. (Stewie's Tweaks bExplosionsDontPushTakeableItems)", game.blasts_spare_items);
        switch!(app, "grabbed-weighs", "Held Items Weigh", "An item you are holding up with the grab counts toward what you carry, so it can leave you overencumbered. (Stewie's Tweaks bEncumbranceIncludesGrabbedItems)", grabbed_weighs);
        switch!(app, "usable-ammo-first", "Usable Ammo First", "In the Pip-Boy's ammo list, ammo that a weapon you carry can fire comes first. (Stewie's Tweaks bSortEquipableAmmo)", usable_ammo_first);
        switch!(app, "better-flycam", "Better Free Camera", "Free camera (tfc): Space rises, Ctrl sinks, Q flies on (S stops), Z and C turn; the wheel sets speed. (Stewie's Tweaks bBetterFlycam)", better_flycam);
        switch!(app, "recent-markers-red", "New Places Red on the Map", "Places you found since starting NVForever are drawn red on the world map. (Stewie's Tweaks bColorRecentlyAddedMapMarkers)", game.recent_markers_red);
        switch!(app, "compass-quest-height", "Quest Marker Height", "A quest marker on the compass more than a storey above or below you has a short bar over or under it. (Stewie's Tweaks bCompassQuestHeightIndicator)", compass_quest_height);
        switch!(app, "compass-marker-height", "Your Marker's Height", "Your own map marker on the compass, more than a storey above or below you, has a short bar over or under it. (Stewie's Tweaks bPlacedMarkerHeightIndicator)", compass_marker_height);
        switch!(app, "hud-marker-name", "Place Name on the HUD", "The name of the map marker the player faces is shown over the compass. (Stewie's Tweaks bHUDMarkerNameIndicator)", hud_marker_name);
        switch!(app, "hud-region-names", "Region Name on the HUD", "Outdoors, the name of the region you stand in (its map name, as the Pip-Boy's map gives it) is shown over the compass. (Stewie's Tweaks bHUDShowRegionNames)", hud_region_names);
        switch!(app, "move-during-pipboy-raise", "Move While the Pip-Boy Comes Up", "You can keep walking and looking about while the Pip-Boy rises to your eyes; the world stops once it is up. (Stewie's Tweaks bMoveDuringOpenPipboyAnim)", move_during_pipboy_raise);
        switch!(app, "region-borders-message-only", "World Borders: Warning Only", "At a world border (a plugin's border region) you are warned but not stopped. (Stewie's Tweaks bDisableRegionBordersKeepMessage)", region_borders_message_only);
        amount!(app, "marker-name-angle", "Place Name: Widest Angle", "How far off straight ahead, in degrees, a place's name is still shown. (Stewie's Tweaks bHUDMarkerNameIndicator)", 45.0, f32, marker_name_angle);
        switch!(app, "marker-name-hide-unfound", "Place Name: Only Places Found", "Places the player has not found are not named. (Stewie's Tweaks bHUDMarkerNameIndicator)", marker_name_hide_unfound);
        add_choice_list(
            app,
            Setting {
                key: "tweak-marker-name-units",
                group: GROUP,
                label: "Place Name: Distance",
                note: "Whether the place's name comes with how far it is, and in what. (Stewie's Tweaks bHUDMarkerNameIndicator)",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            ChoiceList {
                names: |_| choice_names(&MARKER_NAME_UNITS),
                chosen: |w| MARKER_NAME_UNITS[w.get_resource::<TweakSettings>().map_or(0, |t| t.marker_name_units as usize).min(MARKER_NAME_UNITS.len() - 1)].to_string(),
                pick: take_marker_name_units,
                save: |w| MARKER_NAME_UNITS[w.get_resource::<TweakSettings>().map_or(0, |t| t.marker_name_units as usize).min(MARKER_NAME_UNITS.len() - 1)].to_string(),
                load: take_marker_name_units,
                ask_keep: false,
            },
        );
        switch!(app, "hide-hud-compass", "Hide HUD Key: Health and Compass", "The Hide HUD key (on the Controls page) hides the health bar and compass. (Stewie's Tweaks iHideHUDKey)", hide_hud_compass);
        switch!(app, "hide-hud-ammo", "Hide HUD Key: Action Points and Ammo", "The Hide HUD key hides the action points, condition and ammo. (Stewie's Tweaks iHideHUDKey)", hide_hud_ammo);
        switch!(app, "hide-hud-needs", "Hide HUD Key: Hardcore Needs", "The Hide HUD key hides the Hardcore needs words. (Stewie's Tweaks iHideHUDKey)", hide_hud_needs);
        switch!(app, "hide-hud-sneak", "Hide HUD Key: Sneak", "The Hide HUD key hides the sneak word. (Stewie's Tweaks iHideHUDKey)", hide_hud_sneak);
        switch!(app, "hide-hud-on-load", "Hide HUD Key: Hidden After Loading", "A loaded save starts with the Hide HUD key's parts hidden. (Stewie's Tweaks iHideHUDKey)", hide_hud_on_load);
        switch!(app, "grey-read-notes", "Dim Read Terminal Entries", "Terminal entries whose note you have read are drawn dimmer (not those read before this was on). (Stewie's Tweaks bTerminalGreyReadNotes)", grey_read_notes);
        switch!(app, "remember-item-places", "Remember Item List Places", "The Pip-Boy's item lists open on the row they were left on, each tab its own, and a save keeps them. (Stewie's Tweaks bRememberPipboyScrollPositions)", remember_item_places);
        amount!(app, "player-weapon-volume", "Your Weapon Volume", "How loud the player's shots are, in percent of the recorded sound; 100 changes nothing. (Stewie's Tweaks bScaleWeaponVolume)", 200.0, f32, player_weapon_volume);
        amount!(app, "npc-weapon-volume", "Others' Weapon Volume", "How loud other people's and creatures' shots are, in percent of the recorded sound; 100 changes nothing. (Stewie's Tweaks bScaleWeaponVolume)", 200.0, f32, npc_weapon_volume);
        switch!(app, "keep-fall-height-on-load", "Keep Fall Height on Load", "A save made while falling keeps its fall height, so the landing after loading hurts as it would. (Stewie's Tweaks bKeepFallHeightOnLoad)", keep_fall_height_on_load);
        switch!(app, "no-marker-popup", "No Place Marker Question", "Place Marker on the Pip-Boy's maps asks nothing: it puts your marker there, or takes it away while one is placed. (Stewie's Tweaks bNoPlaceMarkerPopup)", game.no_marker_popup);
        switch!(app, "marker-shift-removes", "No Question: Shift Removes", "With No Place Marker Question, Place Marker always puts the marker there; with Shift held it takes it away. (Stewie's Tweaks bPlaceMarkerShiftToReset)", game.marker_shift_removes);
        switch!(app, "marker-removed-on-it", "No Question: Remove When On It", "With No Place Marker Question, Place Marker always puts the marker there, and takes it away when the highlight box is on it. (Stewie's Tweaks bResetIfHovered)", game.marker_removed_on_it);
        amount!(app, "clear-marker-within", "Clear Marker When Near", "Your map marker goes when you come this near it, in game units (a door is about 100 across); 0 keeps it. (Stewie's Tweaks bClearNearbyPlayerMarker)", 2000.0, f32, clear_marker_within);
        switch!(app, "place-marker-here-says", "Place Marker Here Says So", "Place Marker Here (Controls page, unbound at first) marks the map where you stand; this says so. (Stewie's Tweaks iPlaceMapMarkerAtPlayerPosKey)", place_marker_here_says);
        switch!(app, "repeat-activate-takes-only", "Repeat Activate Takes Only", "Repeat Activate (Controls page, unbound at first), held, takes again and again; off, activates anything. (Stewie's Tweaks iRepeatActivateKey)", repeat_activate_takes_only);
        switch!(app, "hi-res-npc-weapons", "Fine Weapon Models for Everyone", "People hold the detailed first-person models of their weapons; weapons drawn after the change. (Stewie's Tweaks bForceHiResWeaponModels)", hi_res_npc_weapons);
        switch!(app, "empty-reload-swaps-ammo", "Reload Swaps Spent Ammo", "Reloading when none of the ammo in use is left changes to another kind you carry. (Stewie's Tweaks bReloadingWithNoAmmoSwapsAmmoTypes)", game.empty_reload_swaps_ammo);
        switch!(
            app,
            "keep-chosen-ammo",
            "Keep Chosen Ammo",
            "Weapons keep the ammo type picked for them; falls back if none is carried. (Stewie's Tweaks bRememberWeaponAmmos)",
            keep_chosen_ammo
        );
        switch!(app, "objectives-on-arrival", "Objectives on Arrival", "Going through a door to a new place shows your active quest's open objectives again. (Stewie's Tweaks bShowQuestObjectivesOnCellChange)", objectives_on_arrival);
        amount!(app, "objectives-gap", "Objectives on Arrival Gap (s)", "The least time, up to 300 seconds, between two showings of Objectives on Arrival. (Stewie's Tweaks bShowQuestObjectivesOnCellChange)", 300.0, f32, objectives_gap);
        // ---- rest ----
        switch!(app, "sleep-in-owned-beds", "Sleep in Owned Beds", "A bed that belongs to someone else can be slept in. (Stewie's Tweaks bSleepInOwnedBeds)", game.sleep_in_owned_beds);
        switch!(app, "faster-sleep-wait", "Faster Sleep and Wait", "Hours count down faster while sleeping or waiting. (Stewie's Tweaks bFasterSleepWait)", game.faster_sleep_wait);
        // ---- items ----
        switch!(app, "power-armor-no-training", "Power Armor Without Training", "Power armor can be worn without Power Armor Training. (Stewie's Tweaks bPowerArmorNeedsNoTraining)", game.power_armor_needs_no_training);
        // ---- world ----
        switch!(app, "no-lock-encounter-zone-levels", "No Encounter Zone Level Lock", "Encounter zones recompute their level from the player's current level instead of freezing on first visit. (Stewie's Tweaks bNoLockEncounterZoneLevels)", game.no_lock_encounter_zone_levels);
        // ---- menus ----
        switch!(app, "wasd-as-arrow-keys", "WASD as Arrow Keys in Menus", "Use WASD as arrow keys and Space to accept across Barter, Quantity, Sleep/Wait, and Recipe menus. (Stewie's Tweaks bUseWASDAsArrowKeys)", game.wasd_as_arrow_keys);
    }
}

/// The Controller Trigger Dead Zones tweak: how far LT and RT go down before they count
/// as pressed, set on each controller's settings (Bevy's own press point, 0.75, with the
/// tweak off); released a little under that [design: docs/design/tweaks.md].
fn trigger_dead_zones(tweaks: Res<TweakSettings>, mut pads: Query<&mut bevy::input::gamepad::GamepadSettings>) {
    use bevy::input::gamepad::{ButtonSettings, GamepadButton};
    let (lt, rt) = if tweaks.custom_trigger_dead_zones { (tweaks.lt_dead_zone, tweaks.rt_dead_zone) } else { (0.75, 0.75) };
    for mut s in &mut pads {
        for (button, press) in [(GamepadButton::LeftTrigger2, lt), (GamepadButton::RightTrigger2, rt)] {
            let press = press.clamp(0.02, 1.0);
            if s.button_settings.get(&button).is_some_and(|b| (b.press_threshold() - press).abs() < 1e-4) {
                continue;
            }
            if let Ok(b) = ButtonSettings::new(press, press * 0.85) {
                s.button_settings.insert(button, b);
            }
        }
    }
}

/// The Last Weapon key: the weapon the player held before the one now is equipped
/// again (put away, as a hotkey's new weapon is) [design: docs/design/tweaks.md]. The weapons are followed
/// each frame: `(now, before)`.
fn last_weapon(input: crate::bindings::ControlInput, state: Res<State<crate::PlayState>>, mut sim: ResMut<crate::sim::SimState>, mut control: ResMut<crate::control::PlayerControl>, mut seen: Local<(Option<fnv_sim::FormId>, Option<fnv_sim::FormId>)>) {
    use fnv_sim::FormId;
    let forms = sim.forms;
    let equipped = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.equipped()).unwrap_or_default();
    let now = equipped.into_iter().find(|&id| forms.item(id).is_some_and(|it| it.kind == fnv_sim::forms::ItemKind::Weapon));
    if now != seen.0 {
        if seen.0.is_some() {
            seen.1 = seen.0;
        }
        seen.0 = now;
    }
    if *state.get() != crate::PlayState::Playing || !input.just_pressed(crate::bindings::Control::LastWeapon) {
        return;
    }
    let Some(before) = seen.1.filter(|&w| sim.world.item_count(FormId::PLAYER_REF, w).is_ok_and(|n| n > 0)) else {
        info!("tweaks: Last Weapon: none to go back to");
        return;
    };
    match sim.world.equip(FormId::PLAYER_REF, before) {
        Ok(_) => {
            control.weapon_out = false;
            info!("tweaks: Last Weapon: {before} equipped (key)");
        }
        Err(e) => info!("tweaks: Last Weapon: {before} not equipped: {e}"),
    }
}

/// The Radio Volume Up and Down keys: the Settings page's Radio volume a tenth either
/// way, between none and whole [design: docs/design/tweaks.md].
fn radio_volume_keys(input: crate::bindings::ControlInput, volumes: Option<Res<crate::audio_out::SoundVolumes>>) {
    use crate::audio_out::SoundCategory;
    use crate::bindings::Control;
    let Some(v) = volumes else { return };
    let step = if input.just_pressed(Control::RadioVolumeUp) {
        0.1
    } else if input.just_pressed(Control::RadioVolumeDown) {
        -0.1
    } else {
        return;
    };
    let level = ((v.get(SoundCategory::Radio) + step) * 10.0).round() / 10.0;
    v.set(SoundCategory::Radio, level.clamp(0.0, 1.0));
    info!("tweaks: Radio volume {:.0}% (key)", v.get(SoundCategory::Radio) * 100.0);
}

/// The Drop Weapon key: the equipped weapon falls in front of the player as the
/// Pip-Boy's Drop puts it (a new reference turned as the player faces, `RefChanged`)
/// [design: docs/design/tweaks.md]. Not one that can't be dropped (a quest's).
fn drop_weapon(input: crate::bindings::ControlInput, state: Res<State<crate::PlayState>>, mut sim: ResMut<crate::sim::SimState>, mut changed: MessageWriter<crate::RefChanged>) {
    use fnv_sim::FormId;
    if *state.get() != crate::PlayState::Playing || !input.just_pressed(crate::bindings::Control::DropWeapon) {
        return;
    }
    let forms = sim.forms;
    let equipped = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.equipped()).unwrap_or_default();
    let Some(item) = equipped.into_iter().find(|&id| forms.item(id).is_some_and(|it| it.kind == fnv_sim::forms::ItemKind::Weapon)) else {
        info!("tweaks: Drop Weapon: nothing in hand");
        return;
    };
    if forms.item(item).is_some_and(|it| it.cant_drop || it.quest_item) {
        info!("tweaks: Drop Weapon: {item} can't be dropped");
        return;
    }
    match sim.world.drop_equipped(FormId::PLAYER_REF, item) {
        Ok(refs) => {
            let heading = sim.world.peek(FormId::PLAYER_REF).map_or(0.0, |p| p.rot[2]);
            for id in refs {
                if let Ok(s) = sim.world.get(id) {
                    s.rot = [0.0, 0.0, heading];
                }
                info!("tweaks: Drop Weapon: {item} dropped as reference {id}");
                changed.write(crate::RefChanged::new(id.0));
            }
        }
        Err(e) => info!("tweaks: Drop Weapon: {item} not dropped: {e}"),
    }
}

/// Show Objectives on Arrival: a new place (another interior, or out to a worldspace)
/// shows the active quest's open objectives again, as a new objective would, at most
/// once in [`TweakSettings::objectives_gap`] seconds [design: docs/design/tweaks.md].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn objectives_on_arrival(
    time: Res<Time>,
    tweaks: Res<TweakSettings>,
    state: Res<State<crate::PlayState>>,
    location: Option<Res<crate::Location>>,
    data: Option<Res<crate::pipboy::data::DataState>>,
    script: Option<Res<ScriptState>>,
    quests: Option<ResMut<crate::quest_messages::QuestMessages>>,
    mut last: Local<(Option<(u32, Option<u32>)>, f32)>,
) {
    let Some(loc) = location else { return };
    let here = (loc.world, loc.interior);
    let was = last.0.replace(here);
    if !tweaks.objectives_on_arrival || *state.get() != crate::PlayState::Playing || was.is_none_or(|w| w == here) {
        return;
    }
    let now = time.elapsed_secs();
    if last.1 > 0.0 && now - last.1 < tweaks.objectives_gap {
        return;
    }
    let (Some(rt), Some(mut quests)) = (script.as_deref().and_then(|s| s.rt.as_ref()), quests) else { return };
    let Some(q) = data.as_deref().and_then(|d| d.active_quest(Some(&rt.game))) else { return };
    let Some(state) = rt.game.quests.get(&fnv_sim::FormId(q)) else { return };
    let list: Vec<crate::script_events::Objective> = state
        .objectives
        .iter()
        .filter(|(_, o)| o.displayed && !o.completed)
        .filter_map(|(i, _)| rt.game.objective_text(fnv_sim::FormId(q), *i).map(|t| rt.game.expand_codes(&t)))
        .filter(|t| !t.trim().is_empty())
        .map(|t| crate::script_events::Objective { quest: q, text: t.trim().to_string(), completed: false })
        .collect();
    if !list.is_empty() {
        last.1 = now;
        info!("tweaks: {} objective(s) of the active quest shown on arrival", list.len());
        quests.push(crate::script_events::QuestNotice::Objectives(list));
    }
}

/// Revive Unconscious Companions on Cell Change (`bReviveWhenChangingCells`): moving to a
/// new interior cell or worldspace revives every knocked-out companion without consuming a
/// Stimpak [design: docs/design/tweaks.md].
fn revive_companions_on_arrival(
    tweaks: Res<TweakSettings>,
    state: Res<State<crate::PlayState>>,
    location: Option<Res<crate::Location>>,
    mut script: Option<ResMut<ScriptState>>,
    sim: Option<ResMut<crate::sim::SimState>>,
    mut last: Local<Option<(u32, Option<u32>)>>,
) {
    let Some(loc) = location else { return };
    let here = (loc.world, loc.interior);
    let was = last.replace(here);
    if !tweaks.game.revive_companions || !tweaks.game.revive_on_cell_change || *state.get() != crate::PlayState::Playing || was.is_none_or(|w| w == here) {
        return;
    }
    let (Some(rt), Some(mut sim)) = (script.as_deref_mut().and_then(|s| s.rt.as_mut()), sim) else { return };
    let revived = crate::script::with_world(rt, &mut sim, |rt| rt.game.revive_companions_on_cell_change());
    if !revived.is_empty() {
        info!("tweaks: {} knocked-out companion(s) revived on cell change", revived.len());
    }
}

/// The Toggle Sighting Node key while playing: the iron sights move to the weapon's
/// second sighting node, or back [design: docs/design/tweaks.md].
fn toggle_sight(input: crate::bindings::ControlInput, state: Res<State<crate::PlayState>>) {
    if *state.get() == crate::PlayState::Playing && input.just_pressed(crate::bindings::Control::ToggleSightingNode) {
        let on = !SECOND_SIGHT.load(std::sync::atomic::Ordering::Relaxed);
        SECOND_SIGHT.store(on, std::sync::atomic::Ordering::Relaxed);
        info!("tweaks: iron sights on the {} sighting node", if on { "second" } else { "first" });
    }
}

/// Seconds between the Repeat Activate key's activations [design: docs/design/tweaks.md].
const REPEAT_ACTIVATE_SECS: f32 = 0.25;

/// The Repeat Activate key (`crate::bindings::Control::RepeatActivate`), held while
/// playing, activates what is under the crosshair every [`REPEAT_ACTIVATE_SECS`] as E
/// would (`crate::activate::ActivateNext`); only things to take when the tweak's switch
/// says so [design: docs/design/tweaks.md].
fn repeat_activate(
    input: crate::bindings::ControlInput,
    time: Res<Time>,
    state: Res<State<crate::PlayState>>,
    tweaks: Res<TweakSettings>,
    target: Option<Res<crate::activate::Target>>,
    next: Option<ResMut<crate::activate::ActivateNext>>,
    mut wait: Local<f32>,
) {
    if *state.get() != crate::PlayState::Playing || !input.pressed(crate::bindings::Control::RepeatActivate) {
        *wait = 0.0;
        return;
    }
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    let Some(t) = target.as_ref().and_then(|t| t.0.as_ref()) else { return };
    if tweaks.repeat_activate_takes_only && !matches!(t.action, crate::activate::outcome::Action::Take { .. }) {
        return;
    }
    if let Some(mut n) = next.filter(|n| n.0.is_none()) {
        n.0 = Some(t.form_id);
        *wait = REPEAT_ACTIVATE_SECS;
        info!("tweaks: Repeat Activate on 0x{:08X} {}", t.form_id, t.name);
    }
}

/// The player's space (an interior cell, else the worldspace) and feet, while playing.
fn player_spot(state: &State<crate::PlayState>, loc: Option<&crate::Location>, player: Option<&crate::player::Player>) -> Option<(u32, [f32; 3])> {
    let (loc, player) = (loc?, player?);
    (*state.get() == crate::PlayState::Playing).then(|| (loc.interior.unwrap_or(loc.world), player.feet))
}

/// The Place Marker Here key (`crate::bindings::Control::PlaceMarkerHere`) puts the
/// custom map marker where the player stands, in the game's copy the Pip-Boy, scripts
/// and the save read (`crate::pipboy::data::marker`) [design: docs/design/tweaks.md].
fn place_marker_here(
    input: crate::bindings::ControlInput,
    state: Res<State<crate::PlayState>>,
    tweaks: Res<TweakSettings>,
    (loc, player): (Option<Res<crate::Location>>, Option<Res<crate::player::Player>>),
    mut script: Option<ResMut<ScriptState>>,
    mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>,
) {
    if !input.just_pressed(crate::bindings::Control::PlaceMarkerHere) {
        return;
    }
    let Some((space, feet)) = player_spot(&state, loc.as_deref(), player.as_deref()) else { return };
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let m = crate::pipboy::data::marker::CustomMarker { space, pos: feet };
    rt.game.other.jg8.custom_marker = Some(m.to_game());
    info!("tweaks: Place Marker Here at ({:.0}, {:.0}) in {}", feet[0], feet[1], fnv_sim::FormId(space));
    if tweaks.place_marker_here_says {
        if let Some(c) = corner.as_deref_mut() {
            // NVForever's own words [design].
            c.push(crate::script_events::Note::from("Your marker is placed here.".to_string()));
        }
    }
}

/// The Clear Marker When Near tweak: the custom map marker goes when the player comes
/// The Hide HUD key (`crate::bindings::Control::HideHud`) hides the parts of the HUD its
/// switches choose, or shows them again (`crate::hud::HudHidden`); at the start, under
/// Hidden After Loading, they start hidden [design: docs/design/tweaks.md].
/// The player's Toggle Controller If Attack Pressed tweak: with the keyboard and mouse
/// working the controls, a controller's A switches them to the controller; with the
/// controller, a left click switches them back. Written to the INI only with the controls'
/// next save (`crate::bindings::with_controls`) [design: docs/design/tweaks.md].
#[cfg(not(target_os = "android"))]
fn toggle_device(tweaks: Res<TweakSettings>, mouse: Res<ButtonInput<MouseButton>>, pads: Query<&Gamepad>, bindings: Option<ResMut<crate::bindings::Bindings>>, listening: Option<Res<crate::pause::Listening>>) {
    use crate::bindings::InputDevice;
    let Some(mut b) = bindings else { return };
    // Not while the Controls tab waits for a new key or button: that is a binding.
    if !tweaks.toggle_device || listening.is_some_and(|l| l.0.is_some()) {
        return;
    }
    let want = match b.device {
        InputDevice::KeyboardMouse if pads.iter().any(|g| g.just_pressed(GamepadButton::South)) => InputDevice::Controller,
        InputDevice::Controller if mouse.just_pressed(MouseButton::Left) => InputDevice::KeyboardMouse,
        _ => return,
    };
    info!("controls: {} now works the controls (Toggle Controller If Attack Pressed)", want.name());
    b.device = want;
}

fn hide_hud_key(input: crate::bindings::ControlInput, state: Res<State<crate::PlayState>>, tweaks: Res<TweakSettings>, hidden: Option<ResMut<crate::hud::HudHidden>>, mut started: Local<bool>) {
    let Some(mut hidden) = hidden else { return };
    if !std::mem::replace(&mut *started, true) {
        hidden.on = tweaks.hide_hud_on_load;
    }
    if *state.get() == crate::PlayState::Playing && input.just_pressed(crate::bindings::Control::HideHud) {
        hidden.on = !hidden.on;
        info!("hud: the Hide HUD key {} its parts (tweak)", if hidden.on { "hides" } else { "shows" });
    }
    let parts = (tweaks.hide_hud_compass, tweaks.hide_hud_ammo, tweaks.hide_hud_needs, tweaks.hide_hud_sneak);
    if (hidden.compass, hidden.ammo, hidden.needs, hidden.sneak) != parts {
        (hidden.compass, hidden.ammo, hidden.needs, hidden.sneak) = parts;
    }
}

/// The Skip Radio Song key (`crate::bindings::Control::SkipRadioSong`) asks the Pip-Boy's
/// station to end its line (`crate::radio::Radio::skip`) [design: docs/design/tweaks.md].
fn skip_radio_song(input: crate::bindings::ControlInput, state: Res<State<crate::PlayState>>, radio: Option<ResMut<crate::radio::Radio>>) {
    if *state.get() == crate::PlayState::Playing && input.just_pressed(crate::bindings::Control::SkipRadioSong) {
        if let Some(mut r) = radio {
            r.skip = true;
        }
    }
}

/// A loaded save starts with the Hide HUD key's parts hidden under Hidden After Loading,
/// else shown: the save holds a mark only so that its load comes here (a save made before
/// it leaves the HUD as it was).
pub const HIDE_HUD_SAVE_HOOK: crate::save::SaveHook = crate::save::SaveHook {
    key: "hide_hud",
    save: |_| Some(serde_json::Value::Bool(true)),
    load: |w, _| {
        let on = w.get_resource::<TweakSettings>().is_some_and(|t| t.hide_hud_on_load);
        if let Some(mut h) = w.get_resource_mut::<crate::hud::HudHidden>() {
            h.on = on;
        }
    },
    remap: None,
};

/// Where the player left each of the inventory's lists, by category: the selected row
/// and the first row shown. Kept with the save; used under the Remember Item List Places
/// tweak [design: docs/design/tweaks.md].
#[derive(Resource, Default, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ItemPlaces(pub [(usize, usize); 6]);

impl ItemPlaces {
    fn slot(c: crate::menu::model::Category) -> usize {
        crate::menu::model::Category::ORDER.iter().position(|&o| o == c).unwrap_or(0)
    }

    /// The row and first row kept for `c`, brought within a list of `rows` rows.
    pub fn place(&self, c: crate::menu::model::Category, rows: usize) -> (usize, usize) {
        let (selected, first) = self.0[Self::slot(c)];
        let selected = selected.min(rows.saturating_sub(1));
        (selected, first.min(selected))
    }

    pub fn keep(&mut self, c: crate::menu::model::Category, selected: usize, first: usize) {
        self.0[Self::slot(c)] = (selected, first);
    }
}

/// [`ItemPlaces`] in the save, always (a save made with the tweak off keeps what was
/// last seen); a save without it leaves the places as they are.
pub const ITEM_PLACES_SAVE_HOOK: crate::save::SaveHook = crate::save::SaveHook {
    key: "item_places",
    save: |w| w.get_resource::<ItemPlaces>().and_then(|p| serde_json::to_value(p).ok()),
    load: |w, v| {
        if let (Ok(p), Some(mut places)) = (serde_json::from_value::<ItemPlaces>(v.clone()), w.get_resource_mut::<ItemPlaces>()) {
            *places = p;
        }
    },
    remap: None,
};

/// The Remember Item List Places tweak: while the inventory is open its list's place is
/// kept by category; when it opens, or its category changes (the model starts a new
/// category at the top), the kept place is put back.
fn remember_item_places(tweaks: Res<TweakSettings>, mut active: ResMut<crate::menu::ActiveMenu>, mut places: ResMut<ItemPlaces>, mut seen: Local<Option<crate::menu::model::Category>>) {
    use crate::menu::model::{Mode, Side};
    // Read first, written only when a place is put back: a write each frame marks the
    // menu changed and redraws the Pip-Boy's lists every frame (a click's press and
    // release then meet two rows; smoke pipboy-items-extras).
    let Some(m) = active.0.as_ref().filter(|m| m.mode == Mode::Inventory) else {
        *seen = None;
        return;
    };
    if !tweaks.remember_item_places {
        *seen = None;
        return;
    }
    let i = Side::Player as usize;
    let category = m.category[i];
    if *seen != Some(category) {
        *seen = Some(category);
        let (selected, first) = places.place(category, m.lists[i].rows.len());
        if (selected, first) != (m.selected[i], m.scroll[i]) {
            if let Some(m) = active.0.as_mut() {
                m.selected[i] = selected;
                m.scroll[i] = first;
                m.navigated = m.navigated.wrapping_add(1);
            }
        }
        return;
    }
    let (selected, scroll) = (m.selected[i], m.scroll[i]);
    places.keep(category, selected, scroll);
}

/// within [`TweakSettings::clear_marker_within`] of it, once they have been farther:
/// a marker placed where the player stands stays until they leave and come back
/// [design: docs/design/tweaks.md].
fn clear_near_marker(
    state: Res<State<crate::PlayState>>,
    tweaks: Res<TweakSettings>,
    (loc, player): (Option<Res<crate::Location>>, Option<Res<crate::player::Player>>),
    mut script: Option<ResMut<ScriptState>>,
    mut armed: Local<Option<(crate::pipboy::data::marker::CustomMarker, bool)>>,
) {
    use crate::pipboy::data::marker::{near, CustomMarker};
    let within = tweaks.clear_marker_within;
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let (Some(m), Some((space, feet))) = (rt.game.custom_map_marker().map(CustomMarker::from_game), player_spot(&state, loc.as_deref(), player.as_deref())) else {
        *armed = None;
        return;
    };
    if within <= 0.0 {
        *armed = None;
        return;
    }
    let is_near = near(m, space, feet, within);
    match armed.as_mut() {
        Some((was, far)) if *was == m => {
            if is_near && *far {
                rt.game.other.jg8.custom_marker = None;
                *armed = None;
                info!("tweaks: the custom map marker is cleared, the player is within {within:.0} of it");
            } else if !is_near {
                *far = true;
            }
        }
        _ => *armed = Some((m, !is_near)),
    }
}

/// The player holds their breath through a scope (the Scope Hold Breath tweak): the
/// scope's sway (nvforever `scope_view`) is multiplied by [`TweakSettings::breath_sway`]
/// [design: docs/design/tweaks.md].
pub static BREATH_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the player is asking to hold breath through a scope: `hold_breath` enabled,
/// scope raised, and either Shift held or Activate held with `breath_activate` enabled
/// [design; Stewie's Tweaks 9.65 public changelog / `nvse_stewie_tweaks.ini` `bActivateHoldsBreath`].
fn hold_breath_asked(hold_breath: bool, scope_on: bool, shift_held: bool, breath_activate: bool, activate_held: bool) -> bool {
    hold_breath && scope_on && (shift_held || (breath_activate && activate_held))
}

/// Shift (or Activate when `breath_activate` is on) held with a scope up holds the breath,
/// paying [`TweakSettings::breath_ap`] Action Points a second; with none left it is let
/// go. In god mode nothing is paid.
fn hold_breath(
    time: Res<Time>,
    tweaks: Res<TweakSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    control: crate::bindings::ControlInput,
    scope: Option<Res<crate::aim_zoom::Scope>>,
    mut script: Option<ResMut<ScriptState>>,
    sim: Option<ResMut<crate::sim::SimState>>,
) {
    let asked = hold_breath_asked(
        tweaks.hold_breath,
        scope.is_some_and(|s| s.on),
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        tweaks.breath_activate,
        control.pressed(crate::bindings::Control::Activate),
    );
    let mut held = asked;
    if asked && !crate::combat::god_mode() && tweaks.breath_ap > 0.0 {
        if let (Some(rt), Some(mut sim)) = (script.as_deref_mut().and_then(|s| s.rt.as_mut()), sim) {
            let cost = f64::from(tweaks.breath_ap * time.delta_secs());
            held = crate::script::with_world(rt, &mut sim, |rt| {
                use fnv_game::actor_values::ACTION_POINTS;
                let p = fnv_sim::FormId::PLAYER_REF;
                let ap = fnv_game::functions::actor_value(&mut rt.game, p, ACTION_POINTS).unwrap_or(0.0);
                if ap > 0.0 {
                    fnv_game::functions::add_actor_value_damage(&mut rt.game, p, ACTION_POINTS, cost.min(ap));
                }
                ap > 0.0
            });
        }
    }
    if BREATH_HELD.swap(held, std::sync::atomic::Ordering::Relaxed) != held {
        info!("tweaks: breath {}", if held { "held through the scope" } else { "let go" });
    }
}

/// Resolve jump AP cost under `bJumpingCostsAP` (`iJumpAPCost`, `bCombatOnly` danger check,
/// `bJumpWithoutEnoughAP`) and `bOverencumberedTweak` (`over_jump_ap`), returning
/// `(ap_damage, jump_allowed)` [design; Stewie's Tweaks 9.70 public changelog /
/// `nvse_stewie_tweaks.ini`].
fn resolve_jump_ap(
    ap: f64,
    base_jump: f32,
    danger_only: bool,
    in_danger: bool,
    allow_without_enough: bool,
    over_jump: f32,
) -> (f64, bool) {
    let base = if danger_only && !in_danger { 0.0 } else { base_jump.max(0.0) };
    let over = over_jump.max(0.0);
    let cost = f64::from(base + over);
    if cost <= 0.0 {
        return (0.0, true);
    }
    if ap >= cost {
        (cost, true)
    } else if allow_without_enough && over <= 0.0 {
        (ap.max(0.0), true)
    } else {
        (0.0, false)
    }
}

/// The Action Point costs of jumping and running ([`TweakSettings::jump_ap_cost`],
/// [`TweakSettings::run_ap_per_second`]) [design: docs/design/tweaks.md]: a jump is paid
/// when asked for and refused without the points (unless `jump_without_enough_ap` is on);
/// running drains each frame and, with none left, the player walks. Not in god mode.
fn ap_costs(
    time: Res<Time>,
    tweaks: Res<TweakSettings>,
    mut intent: ResMut<crate::player::MoveIntent>,
    mut script: Option<ResMut<ScriptState>>,
    sim: Option<ResMut<crate::sim::SimState>>,
    mut jumped: Local<bool>,
) {
    // Over the carry limit under the Overencumbered Running tweak: its own costs too, and
    // without the points the player walks as without the tweak [design: docs/design/tweaks.md].
    let over = tweaks.run_overencumbered && crate::player::OVERENCUMBERED.load(std::sync::atomic::Ordering::Relaxed);
    let over_jump = if over { tweaks.over_jump_ap } else { 0.0 };
    let run = tweaks.run_ap_per_second + if over { tweaks.over_run_ap } else { 0.0 };
    let sneak_run_base = tweaks.sneak_run_ap_base;
    let asked = intent.jump;
    let fresh_jump = asked && !*jumped;
    *jumped = asked;
    if (tweaks.jump_ap_cost <= 0.0 && over_jump <= 0.0 && run <= 0.0 && sneak_run_base <= 0.0) || crate::combat::god_mode() {
        return;
    }
    let (Some(rt), Some(mut sim)) = (script.as_deref_mut().and_then(|s| s.rt.as_mut()), sim) else { return };
    let moving = intent.forward != 0.0 || intent.right != 0.0;
    let running = moving && !intent.walk && !intent.sneak;
    let sneak_running = moving && !intent.walk && intent.sneak;
    let dt = time.delta_secs();
    let intent = &mut *intent;
    crate::script::with_world(rt, &mut sim, |rt| {
        use fnv_game::actor_values::{ACTION_POINTS, ENDURANCE};
        let g = &mut rt.game;
        let p = fnv_sim::FormId::PLAYER_REF;
        let mut ap = fnv_game::functions::actor_value(g, p, ACTION_POINTS).unwrap_or(0.0);
        if fresh_jump && (tweaks.jump_ap_cost > 0.0 || over_jump > 0.0) {
            let in_danger = crate::hud::sneak_now(g) == crate::hud::Sneak::Danger;
            let (drain, allowed) = resolve_jump_ap(
                ap,
                tweaks.jump_ap_cost,
                tweaks.jump_ap_danger_only,
                in_danger,
                tweaks.jump_without_enough_ap,
                over_jump,
            );
            if drain > 0.0 {
                fnv_game::functions::add_actor_value_damage(g, p, ACTION_POINTS, drain);
                ap -= drain;
            }
            if !allowed {
                intent.jump = false;
                let need = tweaks.jump_ap_cost + over_jump;
                info!("tweaks: jump refused, {ap:.0} AP of {need:.0}");
            }
        }
        let move_rate = if running {
            run
        } else if sneak_running {
            // [design]: Endurance fallback of 5.0 when the actor value is unavailable;
            // sneak_run_ap_rate returns 0.0 when base <= 0.0 and clamps at 0.0.
            let endurance = fnv_game::functions::actor_value(g, p, ENDURANCE).unwrap_or(5.0) as f32;
            sneak_run_ap_rate(sneak_run_base, tweaks.sneak_run_ap_endurance_mult, endurance)
        } else {
            0.0
        };
        if move_rate > 0.0 {
            if ap > 0.0 {
                fnv_game::functions::add_actor_value_damage(g, p, ACTION_POINTS, f64::from(move_rate * dt).min(ap));
            } else {
                intent.walk = true;
            }
        }
    });
}

/// The Tweaks tab's keys on the Controls page (`crate::bindings::Control`), none bound
/// unless the player binds them [design: docs/design/tweaks.md]: the crosshair and the sneak label shown or hidden,
/// the menus hidden as ToggleMenus does, the main menu or quit at once, and no collision
/// while held. Only while playing.
#[allow(clippy::too_many_arguments)]
fn keys(
    input: crate::bindings::ControlInput,
    state: Res<State<crate::PlayState>>,
    mut tweaks: ResMut<TweakSettings>,
    mut script: Option<ResMut<ScriptState>>,
    noclip: Option<ResMut<crate::player::NoClip>>,
    mut to_main: MessageWriter<crate::pause::MainMenuRequest>,
    mut exit: MessageWriter<AppExit>,
    (pip, mut inventory): (Option<ResMut<crate::pipboy::PipBoy>>, MessageWriter<crate::menu::OpenInventory>),
) {
    use crate::bindings::Control;
    use crate::pipboy::Page;
    if *state.get() != crate::PlayState::Playing {
        return;
    }
    // The Pip-Boy page and tab keys: open on that page [design: docs/design/tweaks.md].
    const PAGES: [(Control, Page, Option<usize>); 7] = [
        (Control::PipBoyStats, Page::Stats, None),
        (Control::PipBoyItems, Page::Items, None),
        (Control::PipBoyData, Page::Data, None),
        (Control::OpenLocalMap, Page::Data, Some(0)),
        (Control::OpenWorldMap, Page::Data, Some(1)),
        (Control::OpenQuests, Page::Data, Some(2)),
        (Control::OpenRadio, Page::Data, Some(4)),
    ];
    if let Some(mut pip) = pip {
        if let Some(&(_, page, tab)) = PAGES.iter().find(|(c, _, _)| input.just_pressed(*c)) {
            let tab = tab.unwrap_or(pip.tabs[page.index()]);
            pip.open_at = Some((page, tab));
            inventory.write(crate::menu::OpenInventory);
            info!("tweaks: Pip-Boy opened on {} tab {} (key)", page.title(), page.tabs()[tab]);
        }
    }
    if input.just_pressed(Control::SmoothCamera) {
        tweaks.smooth_look = !tweaks.smooth_look;
        info!("tweaks: smooth camera {}", if tweaks.smooth_look { "on" } else { "off" });
    }
    if input.just_pressed(Control::ToggleCrosshair) {
        tweaks.crosshair_hidden = !tweaks.crosshair_hidden;
        info!("tweaks: crosshair {}", if tweaks.crosshair_hidden { "hidden" } else { "shown" });
    }
    if input.just_pressed(Control::ToggleSneakLabel) {
        tweaks.no_sneak_label = !tweaks.no_sneak_label;
        info!("tweaks: sneak label {}", if tweaks.no_sneak_label { "hidden" } else { "shown" });
    }
    if input.just_pressed(Control::ToggleMenus) {
        if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
            let c = &mut rt.game.ui.camera;
            c.menus_hidden = !c.menus_hidden;
            info!("tweaks: menus {}", if c.menus_hidden { "hidden" } else { "shown" });
        }
    }
    if let Some(mut n) = noclip {
        if input.just_pressed(Control::NoCollision) {
            n.0 = true;
            info!("tweaks: no collision (key held)");
        } else if input.just_released(Control::NoCollision) {
            n.0 = false;
            info!("tweaks: collision again");
        }
    }
    if input.just_pressed(Control::ExitToMainMenu) {
        info!("tweaks: back to the main menu (key)");
        to_main.write(crate::pause::MainMenuRequest);
    }
    if input.just_pressed(Control::ExitGame) {
        info!("tweaks: quit (key)");
        exit.write(AppExit::Success);
    }
}

/// The Level Cap slider's top (ours).
const MAX_LEVEL_CAP: f32 = 100.0;

/// What the Pip-Boy says when the No Skill Books Above 100 tweak keeps a book (our words).
pub const BOOK_AT_MAX_LINE: &str = "That skill cannot go any higher.";

/// The corner message when the No Pip-Boy In Combat tweak keeps it shut (our words).
pub const NO_PIPBOY_LINE: &str = "The Pip-Boy cannot be used in combat.";

/// What the Pip-Boy says when the No Healing In Combat tweak refuses an item (our words).
pub const NO_HEALING_LINE: &str = "Healing items cannot be used in combat.";

/// The HUD line when a disarm fails and the mine stays armed (the Disarm Requires Skill
/// tweak) [design: docs/design/tweaks.md].
pub const DISARM_FAILED_LINE: &str = "The disarm failed; the mine is still armed.";

/// The No Healing In Combat tweak: whether using `item` now is refused, as it restores
/// health or a limb and something fights the player (`fighting`,
/// `crate::after_combat::AfterCombat`) [design: docs/design/tweaks.md].
pub fn healing_barred(rt: &mut crate::script::Runtime, sim: &mut crate::sim::SimState, item: fnv_sim::FormId, fighting: bool) -> bool {
    if !fighting || !rt.game.tweaks.no_healing_in_combat {
        return false;
    }
    let gains = crate::script::with_world(rt, sim, |rt| rt.game.aid_preview(fnv_sim::FormId::PLAYER_REF, item));
    gains.iter().any(|&(av, amount)| amount > 0.0 && (av == fnv_game::actor_values::HEALTH || fnv_game::actor_values::LIMB_CONDITIONS.contains(&av)))
}

/// Apply recognized modular Stewie Tweaks overrides (`Data\NVSE\Plugins\Tweaks\INIs\*.ini`)
/// onto `TweakSettings` (both `settings.game` and engine-side fields) [design:
/// docs/spec-notes/stewie-modular-inis.md]. Runs in `PostStartup` before
/// `crate::settings::apply_stored`, so explicit player choices in `NVForever.ini` win
/// over mod-shipped modular INI defaults.
pub fn apply_modular_overrides_to_settings(overrides: &fnv_game::stewie_inis::StewieIniMap, settings: &mut TweakSettings) -> usize {
    if overrides.is_empty() {
        return 0;
    }
    let mut applied = fnv_game::stewie_inis::apply_modular_tweaks_to_game(overrides, &mut settings.game);
    macro_rules! bool_tweak {
        ($key:expr, $($field:ident).+) => {
            if let Some(v) = overrides.get_bool("Tweaks", $key) {
                settings.$($field).+ = v;
                applied += 1;
            }
        };
    }

    bool_tweak!("bNoKarmaMessages", no_karma_messages);
    bool_tweak!("bNoXPMessages", no_xp_messages);
    bool_tweak!("bNoQuestFailedPopup", no_quest_failed_popup);
    bool_tweak!("bNoLocationPopup", no_location_popup);
    bool_tweak!("bRemoveQuestObjectiveAddedText", no_objective_popup);
    bool_tweak!("bDisableFastTravel", disable_fast_travel);
    bool_tweak!("bMidairFastTravel", midair_fast_travel);
    bool_tweak!("bFastTravelWithEnemiesNearby", fast_travel_enemies_nearby);
    bool_tweak!("bFastTravelOverencumbered", fast_travel_overencumbered);
    bool_tweak!("bNoRedCrosshairOnEnemies", no_red_crosshair);
    bool_tweak!("bHideEnemyMarkers", hide_enemy_markers);
    bool_tweak!("bDisableCombatMusic", no_combat_music);
    bool_tweak!("bDisableLoadingScreenTips", no_loading_tips);
    bool_tweak!("bNoReputationMessages", no_reputation_messages);
    bool_tweak!("bDisableExplosionInFaceIMOD", no_explosion_blur);
    bool_tweak!("bRemoveSneakLabel", no_sneak_label);
    bool_tweak!("bVatsExitKey", kill_cam_exit_key);
    bool_tweak!("bNumberedComputerHotkeys", numbered_terminal_keys);
    bool_tweak!("bDeselectHotkeys", deselect_hotkeys);
    bool_tweak!("bNonSelectablePlayerInConsole", console_skips_player);
    bool_tweak!("bPipboyLightAndHolsteringIgnoreTimescale", holster_real_time);
    bool_tweak!("bPowerArmorPreventsScreenBlood", power_armor_no_screen_blood);
    bool_tweak!("bHideRedCrosshairOnDistantInvisibleTargets", no_red_crosshair_on_cloaked);
    bool_tweak!("bGodmodePreventsLegCrippleSound", god_mode_quiet_landing);
    bool_tweak!("bCompressDuplicateConsoleMessages", console_compress_repeats);
    bool_tweak!("bConsolePrintsIncludeTimestamp", console_timestamps);
    bool_tweak!("bLivingAnatomyShowDR", living_anatomy_dr);
    bool_tweak!("bSynchronizeContainerCategories", game.sync_categories);
    bool_tweak!("bNoFastTravelTimeChange", no_travel_time);
    bool_tweak!("bNoFastTravelIfLegsCrippled", no_travel_crippled_leg);
    bool_tweak!("bDisableSellingItems", no_selling);
    bool_tweak!("bAimingSpeed", no_aiming_slowdown);
    bool_tweak!("bRepeatJumping", repeat_jumping);
    bool_tweak!("bHoldCrouchToSneak", hold_to_sneak);
    bool_tweak!("bAddInventoryDropItemHotkey", drop_key);
    bool_tweak!("bAimingUnholstersWeapons", aiming_draws_weapon);
    bool_tweak!("bBarterCheckActorBuySellFlags", game.vendor_lists_both);
    bool_tweak!("bNoPlayerStaggerAnims", no_player_stagger);
    bool_tweak!("bPreventReloadWeaponUnholstering", ready_does_not_draw);
    bool_tweak!("bReloadingPreventsWeaponHotkeys", no_hotkeys_while_reloading);
    bool_tweak!("bMinigamesPlayXPSoundAtMaxLevel", xp_sound_at_cap);
    bool_tweak!("bHoldWaitKeyToShowMenu", hold_to_wait);
    bool_tweak!("bDisableWeaponFOV", no_sight_zoom);
    bool_tweak!("bHideMiscQuestItems", hide_misc_quest);
    bool_tweak!("bClickingActiveTabTogglesSorting", tab_click_sorts);
    bool_tweak!("bHotkeyHolstersWeaponIfEquipped", hotkey_toggles_in_hand);
    bool_tweak!("bDisableInteriorFog", no_interior_fog);
    bool_tweak!("bNoCompassPipsIfNotInDanger", compass_pips_in_danger_only);
    bool_tweak!("bHostilesUseNeutralColorOnCompass", neutral_foes_on_compass);
    bool_tweak!("bShowSneakLabelWhileStanding", sneak_label_standing);
    bool_tweak!("bNoXPBarInCombat", no_xp_meter_in_combat);
    bool_tweak!("bDisableHUDCrippledLimbIndicator", no_limb_indicator);
    bool_tweak!("bSkipDeathcamHotkey", skip_death_cam);
    bool_tweak!("bReplaceHelpWithConsole", help_is_console);
    bool_tweak!("bHeatbeatSoundsFade", heartbeat_fade);
    bool_tweak!("bTerminalFadeTime", terminal_fade);
    bool_tweak!("bContainersShowEquippedAmmo", containers_show_ammo);
    bool_tweak!("bDescriptiveMarkerAddedMessage", descriptive_marker_added);
    bool_tweak!("bDisableGrenadeIndicator", no_grenade_indicator);
    bool_tweak!("bHideGrenadeIndicatorForNoDamageExplosions", grenade_indicator_skip_harmless);
    bool_tweak!("bExplodingDestructibleIndicator", grenade_indicator_destructibles);
    bool_tweak!("bContainerTakeAllDoesntCloseMenu", take_all_stays);
    bool_tweak!("bTakeAllConfirmation", take_all_confirm);
    bool_tweak!("bRepairAllConfirmation", repair_all_confirm);
    bool_tweak!("bHideEquippedItemsInBarter", hide_equipped_barter);
    bool_tweak!("bHideEquippedItemsInContainers", hide_equipped_containers);
    bool_tweak!("bNoCapitaliseContainerCategories", title_case_categories);
    bool_tweak!("bDisableInventoryRightClickDrop", no_right_click_drop);
    bool_tweak!("bPreventNoFastTravelMessage", quiet_undiscovered);
    bool_tweak!("bOnlyChangeCameraHeightIfPOVKeyHeld", camera_wheel_needs_pov);
    bool_tweak!("bNoHotkeyEquipSounds", quiet_hotkeys);
    bool_tweak!("bDisableWeaponHotkeys", no_weapon_hotkeys);
    bool_tweak!("bOpenPipboyToInventoryByDefault", pipboy_opens_items);
    bool_tweak!("bHideCrosshairWhileReloading", hide_crosshair_reloading);
    bool_tweak!("bInvertCameraX", invert_x);
    bool_tweak!("bShowNearestUndiscoveredLocation", nearest_unfound_on_compass);
    bool_tweak!("bLocationDiscoveredCornerMessage", discovered_in_corner);
    bool_tweak!("bInvertContainerTitleScrollwheelDirection", invert_title_wheel);
    bool_tweak!("bAutoContinueGame", auto_continue);
    bool_tweak!("bMainMenuContinueIcon", continue_shows_save);
    bool_tweak!("bQuestTextVisibleInMenus", quest_text_in_menus);
    bool_tweak!("bDetectedByWhom", detected_by_whom);
    bool_tweak!("bKeyRepeatAcceleration", key_repeat_accel);
    bool_tweak!("bHUDFatigueIndicator", hud_fatigue);
    bool_tweak!("bToggleControllerIfAttackPressed", toggle_device);
    bool_tweak!("bScaleExplosionShake", scale_explosion_shake);
    bool_tweak!("bMorePreciseRadMeter", precise_rad_meter);
    bool_tweak!("bShowDoorsOnCompass", compass_doors);
    bool_tweak!("bHideAmmoLabel", hide_ammo_label);
    bool_tweak!("bUseCustomBarterPriceMultipliers", barter_by_kind);
    bool_tweak!("bAllowDuplicateControlBinds", duplicate_binds);
    bool_tweak!("bTogglePOVWhenHolsteringWeapon", pov_follows_weapon);
    bool_tweak!("bContainerEncumbranceIndicator", warn_encumbrance);
    bool_tweak!("bHapticPipboy", pipboy_buzz);
    bool_tweak!("bPowerAttackIfBlocking", power_attack_if_blocking);
    bool_tweak!("bVATSHipFire", vats_hip_fire);
    bool_tweak!("bNoPipBoyInCombat", no_pipboy_in_combat);
    bool_tweak!("bNoEquippedWeaponMovementPenalty", no_weapon_move_penalty);
    bool_tweak!("bUnequipBrokenArmor", unequip_broken_armor);
    bool_tweak!("bHideCrosshairInFirstPerson", hide_crosshair_first);
    bool_tweak!("bHideCrosshairInThirdPerson", hide_crosshair_third);
    bool_tweak!("bBlackJackTotalDisplay", blackjack_total);
    bool_tweak!("bSlotsAutoSpinHotkey", slots_hold_to_spin);
    bool_tweak!("bTalkWhileSneakingIfShiftIsHeld", shift_talks_sneaking);
    bool_tweak!("bTabClosesTerminals", tab_closes_terminals);
    bool_tweak!("bTabBackInStartMenu", tab_back_in_pause);
    bool_tweak!("bEnteringVATSDoesntUnholsterWeapon", vats_keeps_holstered);
    bool_tweak!("bNoHUDHotkeyPopup", no_hotkey_wheel);
    bool_tweak!("bMoveAmmoTypeLabel", ammo_label_under);
    bool_tweak!("bRaceMenuAllowFemaleFacialHair", female_facial_hair);
    bool_tweak!("bContainerMenuStoreAllHotkey", store_all);
    bool_tweak!("bStartMenuQuickLoad", quick_load_in_menus);
    bool_tweak!("bPickLocksEvenWithKey", pick_locks_with_key);
    bool_tweak!("bRightClickChangesToPreviousContainerCategory", right_click_previous_category);
    bool_tweak!("bShowBarterTotalWhenOverSellLimit", full_barter_total);
    bool_tweak!("bCompassFadeLeftSide", compass_fade_left);
    bool_tweak!("bDisableControllerDeadzones", custom_dead_zones);
    bool_tweak!("bControllerTriggerDeadzones", custom_trigger_dead_zones);
    bool_tweak!("bMoreResponsiveControllerAiming", responsive_stick);
    bool_tweak!("bFasterControllerPOVRotate", faster_pad_orbit);
    bool_tweak!("bVanityCamReturnsToFirstPerson", vanity_returns_first);
    bool_tweak!("bRememberConsoleHistory", remember_console_history);
    bool_tweak!("bUseConsoleOutputFile", console_output_file);
    bool_tweak!("bConsoleTextShadow", console_text_shadow);
    bool_tweak!("bClickToExitLoadScreens", click_to_exit_loading);
    bool_tweak!("bBarterShowCapsChange", barter_caps_change);
    bool_tweak!("bJumpSwimsUpwards", jump_swims_up);
    bool_tweak!("bFirstPersonVATS", first_person_vats);
    bool_tweak!("bKeepCrosshairWhenAiming", crosshair_when_aiming);
    bool_tweak!("bMoveDuringVATSPlayback", move_during_kill_cam);
    bool_tweak!("bBetterPickupPrompt", stack_figures);
    bool_tweak!("bTurnFurtherWhileSeated", seated_wide);
    bool_tweak!("bNoTurningAnim", no_turn_clip);
    bool_tweak!("bRemoveLandingAnim", no_land_clip);
    bool_tweak!("bHideInvisibleUndiscoveredLocations", hide_unmapped_places);
    bool_tweak!("bSkipVigorTesterSpecialPages", vigor_review_first);
    bool_tweak!("bHideCursorInMessageMenu", no_pointer_in_message_box);
    bool_tweak!("bHideCursorInDialog", no_pointer_in_dialogue);
    bool_tweak!("bLevelUpScrollWheelSupport", levelup_wheel);
    bool_tweak!("bActivatingDoesntStandUp", seat_keeps_on_activate);
    bool_tweak!("bNoExteriorLoadScreens", plain_going_out);
    bool_tweak!("bHideHotkeyedItemsInContainers", hide_hotkeyed_containers);
    bool_tweak!("bHideHotkeyedItemsInBarter", hide_hotkeyed_barter);
    bool_tweak!("bHideHotkeyedItemsInRepairLists", hide_hotkeyed_repair);
    bool_tweak!("bAddSetHomeCommand", console_homes);
    bool_tweak!("bShiftScreenshotHidesMenus", shift_screenshot_hides);
    bool_tweak!("bOverencumberedTweak", run_overencumbered);
    bool_tweak!("bScopeHoldBreath", hold_breath);
    bool_tweak!("bAdjustableScopeZoom", scope_wheel_zoom);
    bool_tweak!("bMousewheelScrollsWeaponHotkeys", wheel_cycles_hotkeys);
    bool_tweak!("bWeaponCycleUpDownHotkeys", dpad_cycles_hotkeys);
    bool_tweak!("bAllowKeyboardAndMouseWithControllerConnected", keys_with_pad);
    bool_tweak!("bDoubleTapReloadToChangeAmmoType", double_tap_ammo);
    bool_tweak!("bImprovedWeather", improved_weather);
    bool_tweak!("bVATSStopFollowingDeadTargets", vats_stop_following_dead);
    bool_tweak!("bAddNightVisionToggle", night_vision_toggle);
    bool_tweak!("bUseHitLocationInCinematicKillcams", kill_cam_hit_location);
    bool_tweak!("bCompassNPCDistanceBasedAlpha", compass_pips_fade);
    bool_tweak!("bCompassLocationDistanceBasedAlpha", compass_places_fade);
    bool_tweak!("bArmorSoundsPlayIn3D", armor_sounds_3d);
    bool_tweak!("bEncumbranceIncludesGrabbedItems", grabbed_weighs);
    bool_tweak!("bSortEquipableAmmo", usable_ammo_first);
    bool_tweak!("bBetterFlycam", better_flycam);
    bool_tweak!("bForceHiResWeaponModels", hi_res_npc_weapons);
    bool_tweak!("bShowQuestObjectivesOnCellChange", objectives_on_arrival);
    bool_tweak!("bKeepFallHeightOnLoad", keep_fall_height_on_load);
    bool_tweak!("bRememberPipboyScrollPositions", remember_item_places);
    bool_tweak!("bHUDMarkerNameIndicator", hud_marker_name);
    bool_tweak!("bHUDShowRegionNames", hud_region_names);
    bool_tweak!("bMoveDuringOpenPipboyAnim", move_during_pipboy_raise);
    bool_tweak!("bDisableRegionBordersKeepMessage", region_borders_message_only);
    bool_tweak!("bTerminalGreyReadNotes", grey_read_notes);
    bool_tweak!("bCompassQuestHeightIndicator", compass_quest_height);
    bool_tweak!("bPlacedMarkerHeightIndicator", compass_marker_height);
    bool_tweak!("bRememberWeaponAmmos", keep_chosen_ammo);

    if let Some(style) = overrides.get_i32("Tweaks", "iDateFormat").filter(|&s| s >= 0) {
        let max = crate::wait::model::DATE_STYLES.len() - 1;
        settings.date_style = (style as usize).min(max);
        applied += 1;
    }

    applied
}

/// Startup system: load `Data\NVSE\Plugins\Tweaks\INIs\*.ini` into `TweakSettings` before
/// `crate::settings::apply_stored` applies explicit `NVForever.ini` choices.
fn apply_modular_stewie_inis(mut tweaks: ResMut<TweakSettings>) {
    let (files, overrides) = fnv_game::stewie_inis::load_modular_inis_from_data();
    if files == 0 {
        return;
    }
    let applied = apply_modular_overrides_to_settings(&overrides, &mut tweaks);
    info!("tweaks: applied {applied} override(s) from {files} modular Stewie INI file(s)");
}

/// Hand the game's part to the running game when it differs, and log the change.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub(crate) fn sync(
    tweaks: Res<TweakSettings>,
    mut script: Option<ResMut<ScriptState>>,
    quests: Option<ResMut<crate::quest_messages::QuestMessages>>,
    loading: Option<ResMut<crate::loading::Loading>>,
    console: Option<ResMut<crate::console::Console>>,
    mut active: Option<ResMut<crate::menu::ActiveMenu>>,
    bindings: Option<ResMut<crate::bindings::Bindings>>,
    hotkeys: Option<Res<crate::hotkeys::Hotkeys>>,
    (combat, sim, repeat, shake): (Option<Res<crate::combat::Combat>>, Option<Res<crate::sim::SimState>>, Option<ResMut<crate::key_repeat::Acceleration>>, Option<ResMut<crate::shake::ShakeScale>>),
) {
    if let Some(mut s) = shake {
        let want = if tweaks.scale_explosion_shake { tweaks.explosion_shake } else { 1.0 };
        if s.0 != want {
            s.0 = want;
        }
    }
    if let Some(mut r) = repeat {
        let want = crate::key_repeat::Acceleration { on: tweaks.key_repeat_accel, delay: tweaks.key_repeat_delay, mult: tweaks.key_repeat_mult, delay_alt: tweaks.key_repeat_delay_alt, mult_alt: tweaks.key_repeat_mult_alt };
        if *r != want {
            *r = want;
        }
    }
    crate::combat::set_unequip_broken_armor(tweaks.unequip_broken_armor);
    crate::bindings::set_keys_with_pad(tweaks.keys_with_pad);
    crate::terminal::set_grey_read(tweaks.grey_read_notes);
    crate::combat::set_broken_kept(tweaks.game.equip_broken_items, tweaks.game.keep_broken_equipped);
    crate::radiation::set_rad_decimals(if tweaks.precise_rad_meter { tweaks.rad_decimals.max(1) } else { 0 });
    crate::combat::set_weapon_volume(tweaks.player_weapon_volume / 100.0, tweaks.npc_weapon_volume / 100.0);
    crate::kill_cam::set_move_during(tweaks.move_during_kill_cam);
    crate::console::set_model_details(tweaks.console_model_details);
    crate::reticle::set_stack_figures(tweaks.stack_figures);
    SEATED_WIDE.store(tweaks.seated_wide, std::sync::atomic::Ordering::Relaxed);
    NO_TURN_CLIP.store(tweaks.no_turn_clip, std::sync::atomic::Ordering::Relaxed);
    NO_LAND_CLIP.store(tweaks.no_land_clip, std::sync::atomic::Ordering::Relaxed);
    SEAT_KEEPS_ON_ACTIVATE.store(tweaks.seat_keeps_on_activate, std::sync::atomic::Ordering::Relaxed);
    ARMOR_3D.store(tweaks.armor_sounds_3d, std::sync::atomic::Ordering::Relaxed);
    HI_RES_NPC_WEAPONS.store(tweaks.hi_res_npc_weapons, std::sync::atomic::Ordering::Relaxed);
    crate::pad::set_dead_zones(tweaks.custom_dead_zones.then_some((tweaks.left_dead_zone, tweaks.right_dead_zone)));
    crate::wait::model::set_date_style(tweaks.date_style as u8);
    if let Some(mut b) = bindings {
        if b.allow_duplicates != tweaks.duplicate_binds {
            b.allow_duplicates = tweaks.duplicate_binds;
        }
    }
    if let Some(mut l) = loading {
        if l.hide_tips != tweaks.no_loading_tips {
            l.hide_tips = tweaks.no_loading_tips;
        }
        if l.click_to_exit != tweaks.click_to_exit_loading {
            l.click_to_exit = tweaks.click_to_exit_loading;
        }
        if l.plain_going_out != tweaks.plain_going_out {
            l.plain_going_out = tweaks.plain_going_out;
        }
    }
    let hidden = crate::quest_messages::Hidden { failed: tweaks.no_quest_failed_popup, discovered: tweaks.no_location_popup, objectives: tweaks.no_objective_popup };
    // Written without marking the menu changed: a change mark every frame redraws the
    // Pip-Boy's lists every frame, and a click's press and release then meet two
    // different rows (smoke pipboy-items-extras). The rows take the settings at the
    // list's next refresh.
    if let Some(m) = active.as_mut().and_then(|a| a.bypass_change_detection().0.as_mut()) {
        if m.sync_categories != tweaks.game.sync_categories {
            m.sync_categories = tweaks.game.sync_categories;
        }
        if m.prevent_switch_when_emptying_category != tweaks.game.prevent_switch_when_emptying_category {
            m.prevent_switch_when_emptying_category = tweaks.game.prevent_switch_when_emptying_category;
        }
        if m.no_selling != tweaks.no_selling {
            m.no_selling = tweaks.no_selling;
        }
        if m.vendor_lists_both != tweaks.game.vendor_lists_both {
            m.vendor_lists_both = tweaks.game.vendor_lists_both;
        }
        if m.hide_misc_quest != tweaks.hide_misc_quest {
            m.hide_misc_quest = tweaks.hide_misc_quest;
        }
        if m.free_item_price != tweaks.free_item_price {
            m.free_item_price = tweaks.free_item_price;
        }
        let kind_mults = tweaks.barter_by_kind.then_some(crate::menu::model::BarterKindMults {
            sell: [tweaks.barter_sell_armor, tweaks.barter_sell_weapon, tweaks.barter_sell_aid, tweaks.barter_sell_ammo, tweaks.barter_sell_misc, tweaks.barter_sell_weapon_mod],
            buy: [tweaks.barter_buy_armor, tweaks.barter_buy_weapon, tweaks.barter_buy_aid, tweaks.barter_buy_ammo, tweaks.barter_buy_misc, tweaks.barter_buy_weapon_mod],
            water_unscaled: tweaks.barter_water_unscaled,
        });
        if m.barter_kind_mults != kind_mults {
            m.barter_kind_mults = kind_mults;
        }
        if m.warn_encumbrance != tweaks.warn_encumbrance {
            m.warn_encumbrance = tweaks.warn_encumbrance;
        }
        if m.store_all != tweaks.store_all {
            m.store_all = tweaks.store_all;
        }
        if m.full_barter_total != tweaks.full_barter_total {
            m.full_barter_total = tweaks.full_barter_total;
        }
        if m.barter_caps_change != tweaks.barter_caps_change {
            m.barter_caps_change = tweaks.barter_caps_change;
        }
        let confirm = tweaks.take_all_confirm.then_some(tweaks.take_all_confirm_min.max(1.0) as i32);
        if m.take_all_confirm != confirm {
            m.take_all_confirm = confirm;
        }
        let menu = (tweaks.take_all_stays, tweaks.hide_equipped_barter, tweaks.hide_equipped_containers, tweaks.title_case_categories);
        if (m.take_all_stays, m.hide_equipped_barter, m.hide_equipped_containers, m.title_case_categories) != menu {
            (m.take_all_stays, m.hide_equipped_barter, m.hide_equipped_containers, m.title_case_categories) = menu;
        }
        // The ammo the carried weapons fire, for Sort Equipable Ammo.
        let usable: Vec<fnv_sim::FormId> = match (tweaks.usable_ammo_first, combat.as_deref(), sim.as_deref()) {
            (true, Some(c), Some(sim)) => {
                let carried: Vec<fnv_sim::FormId> = sim.world.peek(fnv_sim::FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.groups().map(|g| g.0).collect()).unwrap_or_default();
                let mut out: Vec<fnv_sim::FormId> = carried.iter().filter_map(|w| c.forms.weapons.get(w)).flat_map(|w| c.forms.ammo_choices(w)).collect();
                out.sort_unstable();
                out.dedup();
                out
            }
            _ => Vec::new(),
        };
        if m.usable_ammo != usable {
            m.usable_ammo = usable;
        }
        // The loaded ammo's mark, and the player's tweak for containers.
        let loaded = combat.as_deref().and_then(|c| c.player_ammo());
        // The ammo the weapon in hand can fire: the rest is dimmed in the Pip-Boy (#3442).
        let firable = combat.as_deref().map(|c| c.player_weapon().and_then(|w| c.forms.weapons.get(&w)).map(|w| c.forms.ammo_choices(w)).unwrap_or_default());
        if m.firable_ammo != firable {
            m.firable_ammo = firable;
        }
        if (m.current_ammo, m.ammo_mark_in_containers) != (loaded, tweaks.containers_show_ammo) {
            // The rows take it at the list's next refresh (opening, any change).
            (m.current_ammo, m.ammo_mark_in_containers) = (loaded, tweaks.containers_show_ammo);
        }
        let hotkeyed: Vec<fnv_sim::FormId> = hotkeys.as_deref().map(|h| h.0.iter().flatten().map(|s| fnv_sim::FormId(s.item)).collect()).unwrap_or_default();
        let hide = (tweaks.hide_hotkeyed_containers, tweaks.hide_hotkeyed_barter, tweaks.hide_hotkeyed_repair);
        if (m.hide_hotkeyed_containers, m.hide_hotkeyed_barter, m.hide_hotkeyed_repair) != hide || m.hotkeyed != hotkeyed {
            (m.hide_hotkeyed_containers, m.hide_hotkeyed_barter, m.hide_hotkeyed_repair) = hide;
            m.hotkeyed = hotkeyed;
        }
    }
    if let Some(mut c) = console {
        if (c.compress_repeats, c.timestamps) != (tweaks.console_compress_repeats, tweaks.console_timestamps) {
            c.compress_repeats = tweaks.console_compress_repeats;
            c.timestamps = tweaks.console_timestamps;
        }
        if c.homes_on != tweaks.console_homes {
            c.homes_on = tweaks.console_homes;
        }
        if c.remember_history != tweaks.remember_console_history {
            c.remember(tweaks.remember_console_history);
        }
        if tweaks.console_output_file && !c.auto_output_done {
            c.auto_output();
        }
    }
    if let Some(mut q) = quests {
        if q.hidden != hidden {
            q.hidden = hidden;
        }
    }
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    if rt.game.tweaks != tweaks.game {
        info!("tweaks: the game takes {:?}", tweaks.game);
        rt.game.tweaks = tweaks.game.clone();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn item_places_are_kept_by_category_and_fit_the_list() {
        use crate::menu::model::Category;
        let mut p = super::ItemPlaces::default();
        p.keep(Category::Aid, 12, 7);
        assert_eq!(p.place(Category::Aid, 20), (12, 7));
        assert_eq!(p.place(Category::Aid, 5), (4, 4));
        assert_eq!(p.place(Category::Aid, 0), (0, 0));
        assert_eq!(p.place(Category::Weapons, 20), (0, 0));
        let v = serde_json::to_value(&p).expect("saved");
        assert_eq!(serde_json::from_value::<super::ItemPlaces>(v).expect("loaded"), p);
    }

    use super::*;

    #[test]
    fn the_multiplier_slider_has_one_in_the_middle() {
        assert_eq!(factor(0.5), 1.0);
        assert_eq!(part(1.0), 0.5);
        assert_eq!(shown_factor(0.75), "x1.5");
        assert_eq!(part(9.0), 1.0, "brought into the slider");
    }

    #[test]
    fn the_special_slider_keeps_forty_and_its_ends() {
        assert_eq!(special_total(special_index(40)), 40);
        assert_eq!(special_total(0), SPECIAL_LOW);
        assert_eq!(special_total(SPECIALS - 1), 70);
        assert_eq!(special_index(5), 0, "below the slider");
        assert_eq!(special_index(99), SPECIALS - 1, "above it");
    }

    /// #2377's budget for the Tweaks tab's rows: every note registered here within
    /// `crate::settings::note_over_budget` (200 characters, no command-line switch), the
    /// start's own check (`settings::check_notes`) kept clean.
    #[test]
    fn every_tweak_note_is_within_the_budget() {
        let mut app = App::new();
        TweaksPlugin.build(&mut app);
        let settings = app.world().resource::<crate::settings::Settings>();
        assert!(settings.list().len() > 500, "{}", settings.list().len());
        for s in settings.list() {
            assert_eq!(crate::settings::note_over_budget(s.note), None, "{}: {}", s.key, s.note);
        }
    }

    #[test]
    fn every_tweak_is_off_by_default() {
        let t = TweakSettings::default();
        assert_eq!(t.game, Tweaks::default());
        assert!(!t.game.npc_weapon_mods);
        assert_eq!(t.game.xp_multiplier, 1.0);
        assert!(!t.no_karma_messages && !t.disable_fast_travel && !t.keep_chosen_ammo);
        assert!(!t.breath_activate && !t.jump_ap_danger_only && !t.jump_without_enough_ap);
        assert!(!t.game.sync_categories && t.game.prevent_switch_when_emptying_category);
        assert!(!t.game.no_lock_encounter_zone_levels);
        assert!(!t.game.faster_sleep_wait);
        assert!(!t.game.wasd_as_arrow_keys);
        assert_eq!(t.game.crafted_item_health_pct, 0.0);
        assert_eq!(t.sneak_run_ap_base, 0.0);
        assert_eq!(t.sneak_run_ap_endurance_mult, 0.0);
        assert_eq!(t.console_label(), "Console");
    }

    #[test]
    fn hold_breath_activate_key_requires_parent_scope_and_option() {
        assert!(!hold_breath_asked(false, true, true, true, true));
        assert!(!hold_breath_asked(true, false, true, true, true));
        assert!(hold_breath_asked(true, true, true, false, false));
        assert!(!hold_breath_asked(true, true, false, false, true));
        assert!(hold_breath_asked(true, true, false, true, true));
        assert!(!hold_breath_asked(true, true, false, true, false));
    }

    #[test]
    fn resolve_jump_ap_enforces_danger_gate_and_low_ap_allowance() {
        // Default behavior (danger_only = false, allow_without_enough = false): charges AP and refuses when short.
        assert_eq!(resolve_jump_ap(30.0, 20.0, false, false, false, 0.0), (20.0, true));
        assert_eq!(resolve_jump_ap(10.0, 20.0, false, false, false, 0.0), (0.0, false));
        // Stewie 9.70 danger-only gate (bCombatOnly): free outside danger, charged in danger.
        assert_eq!(resolve_jump_ap(10.0, 20.0, true, false, false, 0.0), (0.0, true));
        assert_eq!(resolve_jump_ap(30.0, 20.0, true, true, false, 0.0), (20.0, true));
        assert_eq!(resolve_jump_ap(10.0, 20.0, true, true, false, 0.0), (0.0, false));
        // Jump without enough AP (bJumpWithoutEnoughAP): drains remaining AP and still allows jump.
        assert_eq!(resolve_jump_ap(10.0, 20.0, false, false, true, 0.0), (10.0, true));
        assert_eq!(resolve_jump_ap(0.0, 20.0, false, false, true, 0.0), (0.0, true));
        // Overencumbered jump AP still enforces sufficient AP.
        assert_eq!(resolve_jump_ap(5.0, 0.0, false, false, true, 10.0), (0.0, false));
        assert_eq!(resolve_jump_ap(15.0, 20.0, true, false, false, 10.0), (10.0, true));
    }

    #[test]
    fn sneak_run_ap_rate_follows_endurance_formula() {
        assert_eq!(sneak_run_ap_rate(0.0, -0.1, 1.0), 0.0, "disabled when base is zero");
        assert_eq!(sneak_run_ap_rate(2.0, 0.0, 10.0), 2.0, "flat base with zero multiplier");
        assert!((sneak_run_ap_rate(2.0, -0.2, 10.0) - 1.0).abs() < 1e-6, "higher Endurance reduces drain");
        assert!((sneak_run_ap_rate(2.0, -0.2, 1.0) - 2.8).abs() < 1e-6, "lower Endurance raises drain");
        assert_eq!(sneak_run_ap_rate(0.5, -0.5, 10.0), 0.0, "clamped non-negative");
    }

    #[test]
    fn help_console_label_uses_custom_text_or_default() {
        let mut t = TweakSettings::default();
        assert_eq!(t.console_label(), "Console");
        t.help_console_label = "   ".to_string();
        assert_eq!(t.console_label(), "Console");
        t.help_console_label = " Dev Console ".to_string();
        assert_eq!(t.console_label(), "Dev Console");
    }

    #[test]
    fn help_console_label_reads_from_nvforever_ini_and_survives_settings_save() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("nvf-help-console-label-{}-{stamp}", std::process::id()));
        let path = dir.join("NVForever.ini");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "[Settings]\ntweak-help-is-console=1\ntweak-help-console-label=\"Dev Console\"\n").unwrap();

        let mut app = App::new();
        app.add_plugins(TweaksPlugin);
        app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        crate::settings::apply_stored(app.world_mut());
        app.world_mut().run_system_cached(read_help_console_label).unwrap();
        {
            let tweaks = app.world().resource::<TweakSettings>();
            assert!(tweaks.help_is_console);
            assert_eq!(tweaks.help_console_label, "Dev Console");
            assert_eq!(tweaks.console_label(), "Dev Console");
        }

        // Saving registered settings preserves the INI-only label line untouched.
        let i = app.world().resource::<crate::settings::Settings>().index("tweak-no-karma-messages").unwrap();
        crate::settings::flip(app.world_mut(), i);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(fnv_install::ini_get(&saved, crate::settings::SECTION, HELP_CONSOLE_LABEL_KEY).as_deref(), Some("\"Dev Console\""), "{saved}");

        // Blank/whitespace value falls back to default "Console".
        std::fs::write(&path, "[Settings]\ntweak-help-console-label=\"   \"\n").unwrap();
        let mut blank = App::new();
        blank.add_plugins(TweaksPlugin);
        blank.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        blank.world_mut().run_system_cached(read_help_console_label).unwrap();
        assert_eq!(blank.world().resource::<TweakSettings>().console_label(), "Console");

        // Unattended run with SettingsFile(None) reads nothing.
        let mut none = App::new();
        none.add_plugins(TweaksPlugin);
        none.insert_resource(crate::settings::SettingsFile(None));
        none.world_mut().run_system_cached(read_help_console_label).unwrap();
        assert_eq!(none.world().resource::<TweakSettings>().console_label(), "Console");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn modular_stewie_inis_apply_sorted_overrides_and_nvforever_ini_wins() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("nvf-play-modular-stewie-{}-{stamp}", std::process::id()));
        let data = root.join("Data");
        let inis = data.join("NVSE/Plugins/Tweaks/INIs");
        std::fs::create_dir_all(&inis).unwrap();

        // Absent/empty check first on a clean root with no modular INIs.
        let empty_root = root.join("EmptyData");
        std::fs::create_dir_all(&empty_root).unwrap();
        let (empty_files, empty_map) = fnv_game::stewie_inis::load_modular_inis_from_roots(&[empty_root]);
        assert_eq!(empty_files, 0);
        let mut unchanged = TweakSettings::default();
        assert_eq!(apply_modular_overrides_to_settings(&empty_map, &mut unchanged), 0);
        assert_eq!(unchanged, TweakSettings::default());

        // Two sorted modular INIs: 20_second.ini overrides 10_first.ini.
        std::fs::write(
            inis.join("10_first.ini"),
            "; first modular override\n\
             [Tweaks]\n\
             bTabBackInStartMenu = 1\n\
             bNoKarmaMessages = 1\n\
             bCritChanceIgnoresFireRate = 1\n\
             bMenuSearch = 1\n\
             bNoLockEncounterZoneLevels = 1\n\
             bFasterSleepWait = 1\n\
             bUseWASDAsArrowKeys = 1\n\
             fCraftedItemHealthPct = 80.0\n\
             iDateFormat = 2\n\
             [Critical Hits]\n\
             fCriticalDamageMult = 1.5\n",
        )
        .unwrap();
        std::fs::write(
            inis.join("20_second.INI"),
            "[tweaks]\n\
             bTabBackInStartMenu = 0 ; second file wins\n\
             bUnknownFutureKey = 1\n\
             [Critical Hits]\n\
             fCriticalDamageMult = 2.0\n",
        )
        .unwrap();

        let (files, overrides) = fnv_game::stewie_inis::load_modular_inis_from_roots(std::slice::from_ref(&data));
        assert_eq!(files, 2);

        let nvf_ini = root.join("NVForever.ini");
        std::fs::write(&nvf_ini, "[Settings]\ntweak-no-karma-messages=0\ntweak-crit-damage-scale=0.25\n").unwrap();

        let mut app = App::new();
        app.add_plugins(TweaksPlugin);
        app.insert_resource(crate::settings::SettingsFile(Some(nvf_ini)));
        {
            let mut settings = app.world_mut().resource_mut::<TweakSettings>();
            apply_modular_overrides_to_settings(&overrides, &mut settings);
            assert!(!settings.tab_back_in_pause, "20_second.INI disabled bTabBackInStartMenu");
            assert!(settings.no_karma_messages);
            assert!(settings.game.crit_ignores_fire_rate);
            assert!(settings.game.menu_search);
            assert!(settings.game.no_lock_encounter_zone_levels);
            assert!(settings.game.faster_sleep_wait);
            assert!(settings.game.wasd_as_arrow_keys);
            assert_eq!(settings.game.crafted_item_health_pct, 80.0);
            assert_eq!(settings.date_style, 2);
            assert_eq!(settings.game.crit_damage_scale, 2.0);
        }
        // Explicit NVForever.ini preferences override modular INI defaults while
        // unmentioned modular keys remain intact.
        crate::settings::apply_stored(app.world_mut());
        let final_settings = app.world().resource::<TweakSettings>();
        assert!(!final_settings.no_karma_messages, "explicit NVForever.ini off wins");
        assert_eq!(final_settings.game.crit_damage_scale, 0.5, "explicit NVForever.ini slider 0.25 -> x0.5 wins");
        assert!(final_settings.game.crit_ignores_fire_rate, "unmentioned modular key stays enabled");
        assert!(final_settings.game.menu_search, "unmentioned modular key stays enabled");
        assert!(final_settings.game.no_lock_encounter_zone_levels, "unmentioned modular key stays enabled");
        assert!(final_settings.game.faster_sleep_wait, "unmentioned modular key stays enabled");
        assert!(final_settings.game.wasd_as_arrow_keys, "unmentioned modular key stays enabled");
        assert_eq!(final_settings.game.crafted_item_health_pct, 80.0, "unmentioned modular key stays applied");
        assert_eq!(final_settings.date_style, 2);
        // Toggling the registered switch in NVForever.ini overrides the modular INI back to off.
        let idx = app.world().resource::<crate::settings::Settings>().index("tweak-no-lock-encounter-zone-levels").unwrap();
        crate::settings::flip(app.world_mut(), idx);
        assert!(!app.world().resource::<TweakSettings>().game.no_lock_encounter_zone_levels, "UI switch toggles off");
        let sleep_idx = app.world().resource::<crate::settings::Settings>().index("tweak-faster-sleep-wait").unwrap();
        crate::settings::flip(app.world_mut(), sleep_idx);
        assert!(!app.world().resource::<TweakSettings>().game.faster_sleep_wait, "UI switch toggles off");
        let wasd_idx = app.world().resource::<crate::settings::Settings>().index("tweak-wasd-as-arrow-keys").unwrap();
        crate::settings::flip(app.world_mut(), wasd_idx);
        assert!(!app.world().resource::<TweakSettings>().game.wasd_as_arrow_keys, "UI switch toggles off");
        let craft_idx = app.world().resource::<crate::settings::Settings>().index("tweak-crafted-item-health-pct").unwrap();
        crate::settings::set_slider(app.world_mut(), craft_idx, 0.5);
        assert_eq!(app.world().resource::<TweakSettings>().game.crafted_item_health_pct, 50.0, "UI slider updates condition pct");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remember_weapon_ammos_ini_maps_to_keep_chosen_ammo_and_respects_nvforever_ini() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("nvf-play-remember-weapon-ammos-{}-{stamp}", std::process::id()));
        let data = root.join("Data");
        let plugins = data.join("NVSE/Plugins");
        let inis = plugins.join("Tweaks/INIs");
        std::fs::create_dir_all(&inis).unwrap();

        // Base nvse_stewie_tweaks.ini alone does not supply NVForever defaults (#3535),
        // keeping unmodded/vanilla keep_chosen_ammo = false while StewieGetValue/query_value
        // still resolves it from the base file.
        std::fs::write(plugins.join("nvse_stewie_tweaks.ini"), "[Tweaks]\nbRememberWeaponAmmos = 1\n").unwrap();
        let (no_mod_files, no_mod_map) = fnv_game::stewie_inis::load_modular_inis_from_roots(std::slice::from_ref(&data));
        assert_eq!(no_mod_files, 0);
        let mut base_only = TweakSettings::default();
        assert_eq!(apply_modular_overrides_to_settings(&no_mod_map, &mut base_only), 0);
        assert!(!base_only.keep_chosen_ammo, "base nvse_stewie_tweaks.ini alone leaves default off");

        let mut mod_files = fnv_game::mod_files::ModFiles::default();
        assert!(mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", b"[Tweaks]\nbRememberWeaponAmmos = 1\n"));
        assert_eq!(fnv_game::stewie_inis::query_value(&mod_files, "bRememberWeaponAmmos", "Tweaks").as_deref(), Some("1"));

        // Invalid boolean value leaves keep_chosen_ammo unchanged (false).
        let mut invalid_map = fnv_game::stewie_inis::StewieIniMap::default();
        invalid_map.parse_into("[Tweaks]\nbRememberWeaponAmmos = not_a_bool\n");
        let mut invalid_settings = TweakSettings::default();
        assert_eq!(apply_modular_overrides_to_settings(&invalid_map, &mut invalid_settings), 0);
        assert!(!invalid_settings.keep_chosen_ammo);

        // Modular Tweaks/INIs/10_enable.ini enables keep_chosen_ammo.
        std::fs::write(inis.join("10_enable.ini"), "; Stewie 10.05 modular override\n[Tweaks]\nbRememberWeaponAmmos = 1\n").unwrap();
        let (files_on, map_on) = fnv_game::stewie_inis::load_modular_inis_from_roots(std::slice::from_ref(&data));
        assert_eq!(files_on, 1);

        // Unmentioned in NVForever.ini: modular bRememberWeaponAmmos=1 stays enabled.
        let nvf_unmentioned = root.join("NVForever-unmentioned.ini");
        std::fs::write(&nvf_unmentioned, "[Settings]\n").unwrap();
        let mut app_on = App::new();
        app_on.add_plugins(TweaksPlugin);
        app_on.insert_resource(crate::settings::SettingsFile(Some(nvf_unmentioned)));
        {
            let mut settings = app_on.world_mut().resource_mut::<TweakSettings>();
            assert_eq!(apply_modular_overrides_to_settings(&map_on, &mut settings), 1);
            assert!(settings.keep_chosen_ammo);
        }
        crate::settings::apply_stored(app_on.world_mut());
        assert!(app_on.world().resource::<TweakSettings>().keep_chosen_ammo);

        // Explicit NVForever.ini off wins over modular bRememberWeaponAmmos=1.
        let nvf_off = root.join("NVForever-off.ini");
        std::fs::write(&nvf_off, "[Settings]\ntweak-keep-chosen-ammo=0\n").unwrap();
        let mut app_user_off = App::new();
        app_user_off.add_plugins(TweaksPlugin);
        app_user_off.insert_resource(crate::settings::SettingsFile(Some(nvf_off)));
        {
            let mut settings = app_user_off.world_mut().resource_mut::<TweakSettings>();
            assert_eq!(apply_modular_overrides_to_settings(&map_on, &mut settings), 1);
            assert!(settings.keep_chosen_ammo);
        }
        crate::settings::apply_stored(app_user_off.world_mut());
        assert!(!app_user_off.world().resource::<TweakSettings>().keep_chosen_ammo, "explicit NVForever.ini off wins");

        // Later modular file setting bRememberWeaponAmmos=0 turns it back off,
        // overrides base INI for query_value, and still yields to explicit NVForever.ini=1.
        std::fs::write(inis.join("20_disable.INI"), "[tweaks]\nbRememberWeaponAmmos = 0\n").unwrap();
        let (files_off, map_off) = fnv_game::stewie_inis::load_modular_inis_from_roots(std::slice::from_ref(&data));
        assert_eq!(files_off, 2);
        let mut disabled = TweakSettings { keep_chosen_ammo: true, ..TweakSettings::default() };
        assert_eq!(apply_modular_overrides_to_settings(&map_off, &mut disabled), 1);
        assert!(!disabled.keep_chosen_ammo, "later modular file with 0 disables keep_chosen_ammo");

        assert!(mod_files.write_bytes("NVSE/Plugins/Tweaks/INIs/20_disable.INI", b"[tweaks]\nbRememberWeaponAmmos = 0\n"));
        assert_eq!(fnv_game::stewie_inis::query_value(&mod_files, "bRememberWeaponAmmos", "Tweaks").as_deref(), Some("0"));

        let nvf_on = root.join("NVForever-on.ini");
        std::fs::write(&nvf_on, "[Settings]\ntweak-keep-chosen-ammo=1\n").unwrap();
        let mut app_user_on = App::new();
        app_user_on.add_plugins(TweaksPlugin);
        app_user_on.insert_resource(crate::settings::SettingsFile(Some(nvf_on)));
        {
            let mut settings = app_user_on.world_mut().resource_mut::<TweakSettings>();
            assert_eq!(apply_modular_overrides_to_settings(&map_off, &mut settings), 1);
            assert!(!settings.keep_chosen_ammo);
        }
        crate::settings::apply_stored(app_user_on.world_mut());
        assert!(app_user_on.world().resource::<TweakSettings>().keep_chosen_ammo, "explicit NVForever.ini on wins");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sim_state_loads_modular_game_settings_into_form_store_and_world() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("nvf-play-modular-gmst-{}-{stamp}", std::process::id()));
        let gmst_dir = root.join("Data/nvse/plugins/tweaks/gamesettings");
        std::fs::create_dir_all(&gmst_dir).unwrap();
        std::fs::write(
            gmst_dir.join("01_base.ini"),
            "; headerless GMST overrides\n\
             fSyntheticFloat = 12.5\n\
             iSyntheticInt = 7\n\
             sSyntheticText = \"First\"\n",
        )
        .unwrap();
        std::fs::write(
            gmst_dir.join("02_override.INI"),
            "[GameSettings]\n\
             iSyntheticInt = 42 ; later file overrides\n\
             sSyntheticText = \"Second;Value\"\n\
             fUnknownGmst = 99.0\n",
        )
        .unwrap();

        let mut forms = fnv_sim::FormStore::default();
        forms.settings.insert("fsyntheticfloat".into(), fnv_sim::settings::Setting::Float(1.0));
        forms.settings.insert("isyntheticint".into(), fnv_sim::settings::Setting::Int(1));
        forms.settings.insert("ssynthetictext".into(), fnv_sim::settings::Setting::Str("Initial".into()));
        let (files, applied) = fnv_game::stewie_inis::apply_modular_game_settings_to_forms_from_roots(&mut forms, &[root.join("Data")]);
        assert_eq!(files, 2);
        assert_eq!(applied, 3);
        assert_eq!(forms.setting_f32("fSyntheticFloat"), Some(12.5));
        assert_eq!(forms.setting_f32("iSyntheticInt"), Some(42.0));
        assert_eq!(forms.settings.get("ssynthetictext"), Some(&fnv_sim::settings::Setting::Str("Second;Value".into())));
        assert!(!forms.settings.contains_key("funknowngmst"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn npc_weapon_mods_setting_defaults_off_persists_and_syncs_to_game_tweaks() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("nvf-npc-weapon-mods-{}-{stamp}", std::process::id()));
        let path = dir.join("NVForever.ini");
        std::fs::create_dir_all(&dir).unwrap();

        let make_app = |ini: &std::path::Path| {
            let mut app = App::new();
            app.add_plugins(TweaksPlugin);
            app.insert_resource(crate::settings::SettingsFile(Some(ini.to_path_buf())));
            app.insert_resource(crate::script::ScriptState::over_no_plugins());
            crate::settings::apply_stored(app.world_mut());
            app.world_mut().run_system_cached(sync).unwrap();
            app
        };

        // 1. Off by default, registered in Gameplay, note within budget, synced false to Game::tweaks.
        let mut app = make_app(&path);
        assert!(!app.world().resource::<TweakSettings>().game.npc_weapon_mods);
        assert!(!app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.tweaks.npc_weapon_mods);
        let idx = app.world().resource::<crate::settings::Settings>().index("npc-weapon-mods").expect("npc-weapon-mods registered");
        let row = &app.world().resource::<crate::settings::Settings>().list()[idx];
        assert_eq!(row.group, crate::pause::GROUP_MENUS);
        assert_eq!(crate::settings::note_over_budget(row.note), None);

        // 2. Flipping ON persists npc-weapon-mods=1 and syncs true into live Game::tweaks.
        assert_eq!(crate::settings::flip(app.world_mut(), idx), Some(true));
        app.world_mut().run_system_cached(sync).unwrap();
        assert!(app.world().resource::<TweakSettings>().game.npc_weapon_mods);
        assert!(app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.tweaks.npc_weapon_mods);
        let ini_on = std::fs::read_to_string(&path).unwrap();
        assert!(ini_on.contains("npc-weapon-mods=1"), "persisted ON: {ini_on}");

        // 3. Reloading restores ON and syncs true to live Game::tweaks.
        let mut reloaded = make_app(&path);
        assert!(reloaded.world().resource::<TweakSettings>().game.npc_weapon_mods);
        assert!(reloaded.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.tweaks.npc_weapon_mods);

        // 4. Flipping OFF persists npc-weapon-mods=0 and syncs false to live Game::tweaks.
        let idx2 = reloaded.world().resource::<crate::settings::Settings>().index("npc-weapon-mods").unwrap();
        assert_eq!(crate::settings::flip(reloaded.world_mut(), idx2), Some(false));
        reloaded.world_mut().run_system_cached(sync).unwrap();
        assert!(!reloaded.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.tweaks.npc_weapon_mods);
        let ini_off = std::fs::read_to_string(&path).unwrap();
        assert!(ini_off.contains("npc-weapon-mods=0"), "persisted OFF: {ini_off}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tweaks_has_no_few_step_sliders_and_four_choice_lists_roundtrip_names_and_legacy_parts() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("nvf-play-tweak-choice-lists-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ini = root.join("NVForever.ini");

        let start = |path: &std::path::Path| {
            let mut app = App::new();
            app.add_plugins(TweaksPlugin);
            app.insert_resource(crate::settings::SettingsFile(Some(path.to_path_buf())));
            crate::settings::apply_stored(app.world_mut());
            app
        };

        let mut app = start(&ini);
        let settings = app.world().resource::<crate::settings::Settings>();
        let few: Vec<&str> = settings.list().iter().filter(|s| crate::settings::few_step_slider(s)).map(|s| s.key).collect();
        assert!(few.is_empty(), "TweaksPlugin must have zero sliders with <= 7 values: {few:?}");

        for (key, expected_default) in [
            ("tweak-perks-per-level", "1"),
            ("tweak-date-style", "As the game"),
            ("tweak-companion-pip", "Off"),
            ("tweak-marker-name-units", "None"),
        ] {
            let i = settings.index(key).unwrap_or_else(|| panic!("missing {key}"));
            assert!(settings.list()[i].slider.is_none(), "{key} must not be a slider");
            let list = settings.choices(i).unwrap_or_else(|| panic!("{key} must be a choice list"));
            assert_eq!((list.chosen)(app.world()), expected_default, "{key} default");
        }

        // `tweak-marker-name-units` is greyed out while `tweak-hud-marker-name` is off.
        let units_idx = settings.index("tweak-marker-name-units").unwrap();
        let hud_name_idx = settings.index("tweak-hud-marker-name").unwrap();
        let perks_idx = settings.index("tweak-perks-per-level").unwrap();
        let date_idx = settings.index("tweak-date-style").unwrap();
        let pip_idx = settings.index("tweak-companion-pip").unwrap();
        assert!(settings.disabled_status(app.world(), units_idx).is_some(), "units greyed until Place Name on the HUD is on");
        assert!(!crate::settings::pick(app.world_mut(), units_idx, "Feet"), "greyed row refuses pick");
        assert_eq!(crate::settings::flip(app.world_mut(), hud_name_idx), Some(true));
        assert!(app.world().resource::<crate::settings::Settings>().disabled_status(app.world(), units_idx).is_none());

        // Pick named choices, verify TweakSettings + INI save + reload round-trip.
        assert!(crate::settings::pick(app.world_mut(), perks_idx, "4"));
        assert!(crate::settings::pick(app.world_mut(), date_idx, "Year first (2281-10-19)"));
        assert!(crate::settings::pick(app.world_mut(), pip_idx, "Cyan"));
        assert!(crate::settings::pick(app.world_mut(), units_idx, "Feet"));
        {
            let t = app.world().resource::<TweakSettings>();
            assert_eq!(t.game.perks_per_level, 4);
            assert_eq!(t.date_style, 3);
            assert_eq!(t.companion_pip, 3);
            assert_eq!(t.marker_name_units, 2);
        }
        let saved = std::fs::read_to_string(&ini).unwrap();
        assert!(saved.contains("tweak-perks-per-level=4"), "{saved}");
        assert!(saved.contains("tweak-date-style=Year first (2281-10-19)"), "{saved}");
        assert!(saved.contains("tweak-companion-pip=Cyan"), "{saved}");
        assert!(saved.contains("tweak-marker-name-units=Feet"), "{saved}");

        let reloaded = start(&ini);
        let t = reloaded.world().resource::<TweakSettings>();
        assert_eq!(t.game.perks_per_level, 4);
        assert_eq!(t.date_style, 3);
        assert_eq!(t.companion_pip, 3);
        assert_eq!(t.marker_name_units, 2);

        // Legacy slider parts from older NVForever.ini files still load to the exact choices.
        std::fs::write(
            &ini,
            "[Settings]\n\
             tweak-perks-per-level=1.00\n\
             tweak-date-style=0.333\n\
             tweak-companion-pip=0.40\n\
             tweak-marker-name-units=0.50\n",
        )
        .unwrap();
        let legacy = start(&ini);
        let t = legacy.world().resource::<TweakSettings>();
        assert_eq!(t.game.perks_per_level, 5, "legacy slider 1.00 -> 5 perks");
        assert_eq!(t.date_style, 1, "legacy slider 0.333 -> Day first");
        assert_eq!(t.companion_pip, 2, "legacy slider 0.40 -> Blue");
        assert_eq!(t.marker_name_units, 1, "legacy slider 0.50 -> Metres");

        // Legacy 0.00 / 0.25 / invalid values.
        std::fs::write(
            &ini,
            "[Settings]\n\
             tweak-perks-per-level=0.25\n\
             tweak-date-style=bogus\n\
             tweak-companion-pip=bogus\n\
             tweak-marker-name-units=bogus\n",
        )
        .unwrap();
        let mixed = start(&ini);
        let t = mixed.world().resource::<TweakSettings>();
        assert_eq!(t.game.perks_per_level, 2, "legacy slider 0.25 -> 2 perks");
        assert_eq!(t.date_style, 0, "invalid value keeps default");
        assert_eq!(t.companion_pip, 0, "invalid value keeps default");
        assert_eq!(t.marker_name_units, 0, "invalid value keeps default");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sound_list_choice_labels_strip_ui_prefix_and_keep_full_edid_storage() {
        let mut app = App::new();
        app.add_plugins(TweaksPlugin);

        let settings = app.world().resource::<crate::settings::Settings>();
        for key in ["tweak-discovered-sound", "tweak-level-sound-a", "tweak-level-sound-b"] {
            let idx = settings.index(key).unwrap_or_else(|| panic!("missing {key}"));
            assert_eq!(settings.choice_label(app.world(), idx, "UILockpickingCylinderSqueakA"), "LockpickingCylinderSqueakA");
            assert_eq!(settings.choice_label(app.world(), idx, "UILockpickingCylinderSqueakB"), "LockpickingCylinderSqueakB");
            assert_eq!(settings.choice_label(app.world(), idx, GAME_SOUND), GAME_SOUND);
            assert_eq!(settings.choice_label(app.world(), idx, "UI"), "UI");
        }

        let idx = app.world().resource::<crate::settings::Settings>().index("tweak-discovered-sound").unwrap();
        assert!(crate::settings::pick(app.world_mut(), idx, "UILockpickingCylinderSqueakA"));
        assert_eq!(app.world().resource::<TweakSettings>().discovered_sound, "UILockpickingCylinderSqueakA");
    }
}
