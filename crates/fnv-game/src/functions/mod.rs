//! Script function implementations, dispatched by opcode (docs/design/layer5.md §3.7).
//! Scripts and conditions share them (a condition's function index is opcode −
//! 0x1000). Each entry is registered with the catalogue name of its opcode, which a test
//! checks against `schemas/script/functions.json`.
//!
//! - [`Kind::Impl`]: implemented over the Layer 4 world and the Layer 5 state;
//! - [`Kind::Stub`]: the function belongs to a system not built yet (or one another
//!   workstream is building, [`stubs::ASSIGNED`]): the call is recorded in the trace and
//!   has no effect;
//! - [`Kind::NoOp`]: nothing to do on this engine (a console's controller, the
//!   original's memory management): handled, no effect, not traced;
//! - not listed: unhandled (traced once per opcode and unit, counted, returns 0).
//!
//! Behaviour notes per function are paraphrased from the catalogue (`summary`/`notes`,
//! our wording of the GECK wiki) unless tagged otherwise. docs/spec-notes/scripts.md §7
//! lists every function with its evidence.

pub(crate) mod actor;
mod ai_state;
pub mod camera;
pub mod console;
mod fighting;
pub(crate) mod fo3;
pub mod toggles;
pub(crate) mod furniture;
pub(crate) mod items;
mod magic;
pub mod misc;
pub mod navigation;
pub(crate) mod objects;
pub use objects::{critical_stage_culls, CRITICAL_DISINTEGRATE_END, CRITICAL_GOO_END};
pub(crate) mod player;
pub(crate) mod player_source;
mod quest;
pub(crate) mod refs;
pub use refs::FADE_OUT_HOLD;
mod reputation;
pub mod senses;
pub mod stubs;
pub mod ui;

pub use reputation::{change as reputation_change, crime_infamy, hide as reputation_hide, real as reputation_real, reputations_of, store as reputation_store, tiers as reputation_tiers};
pub use actor::{actor_value, actor_value_capped, actor_value_parts, temporary as actor_value_temporary, add_damage as add_actor_value_damage, restore as restore_actor_value_damage, av_by_name, av_code, set_current as set_actor_value_current, weapon_anim_code, AV_NAMES};
pub use actor::{faction_rank, faction_relation, factions_of, factions_relation, mine_spares};
pub use misc::clock_now;
pub use objects::scale_of;
pub use player::refuse_power_armor;
pub use refs::{list_members, repair_list};
pub use fighting::cause_code;
pub(crate) use items::equipped_weapon as equipped_weapon_of;
/// The level (0–3) of fame or infamy against a reputation's scale (the Pip-Boy's STATS page).
pub use reputation::level as reputation_level;

use crate::game::{Game, InstKey};
use fnv_script::vm::{ArgValue, Value};
use fnv_sim::FormId;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The context of one call.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The running unit's key (for trace events), or a condition's owner label. Shared:
    /// a script's calls clone its unit's key without copying the text (#1389).
    pub unit: std::sync::Arc<str>,
    pub opcode: u16,
    /// The explicit calling reference (`Ref.Func`), resolved.
    pub on: Option<FormId>,
    /// The reference the script is attached to (object scripts, dialogue speakers,
    /// package owners); a condition's run-on reference.
    pub self_ref: Option<FormId>,
    /// The activating reference inside OnActivate, the holder in inventory events.
    pub action_ref: Option<FormId>,
    /// The quest whose script or stage result is running.
    pub quest: Option<FormId>,
    pub inst: Option<InstKey>,
    pub in_condition: bool,
}

impl Ctx {
    /// The calling reference: explicit, else the script's own reference
    /// [hypothesis: a bare call in an object script runs on its reference,
    /// script-functions.md §6].
    pub fn subject(&self) -> Result<FormId, String> {
        self.on.or(self.self_ref).ok_or_else(|| "no calling reference".to_string())
    }

    /// The calling reference, or the player when there is none (functions documented to
    /// act on the player without a reference).
    pub fn subject_or_player(&self) -> FormId {
        self.on.or(self.self_ref).unwrap_or(FormId::PLAYER_REF)
    }
}

/// An argument an accessor could not read although the call passed one: the parameter
/// (from 0), the kind it held and the kind the accessor wanted. A function that then goes
/// on with a default (`num_or`, `opt_form`, `unwrap_or`) has read the call wrong without
/// failing (#1122: ShowMessage's compiled variables, one `Vars` list, read as numbers
/// became 0); [`Game::call_function`] traces it as `EventKind::ArgUnread`. An absent
/// argument (an optional parameter) is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    pub param: usize,
    pub got: &'static str,
    pub wanted: &'static str,
}

thread_local! {
    static UNREAD: std::cell::RefCell<Vec<Unread>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Start recording unread arguments for one call; returns what an outer call had
/// recorded, to hand back to [`end_unread`].
pub fn begin_unread() -> Vec<Unread> {
    UNREAD.with(|u| std::mem::take(&mut *u.borrow_mut()))
}

/// The unread arguments recorded since [`begin_unread`], the outer call's put back.
pub fn end_unread(outer: Vec<Unread>) -> Vec<Unread> {
    UNREAD.with(|u| std::mem::replace(&mut *u.borrow_mut(), outer))
}

/// The name of an argument's kind in the report.
pub fn arg_kind(a: &ArgValue) -> &'static str {
    match a {
        ArgValue::Value(Value::Ref(_)) => "ref",
        ArgValue::Value(Value::Num(_)) => "number",
        ArgValue::Enum(_) => "code",
        ArgValue::Axis(_) => "axis",
        ArgValue::Str(_) => "string",
        ArgValue::Vars(_) => "message variables",
        ArgValue::Int32(_) => "int32",
    }
}

/// Evaluated arguments with typed accessors. An accessor given an argument of a kind it
/// cannot read records it ([`Unread`]); one given none records nothing.
pub struct Args<'a>(pub &'a [ArgValue]);

impl Args<'_> {
    fn unread(&self, i: usize, wanted: &'static str) {
        if let Some(a) = self.0.get(i) {
            let u = Unread { param: i, got: arg_kind(a), wanted };
            UNREAD.with(|r| {
                let mut r = r.borrow_mut();
                if !r.contains(&u) {
                    r.push(u);
                }
            });
        }
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&ArgValue> {
        self.0.get(i)
    }

    pub fn form(&self, i: usize) -> Result<FormId, String> {
        self.opt_form(i).ok_or_else(|| format!("parameter {} is not a form", i + 1))
    }

    /// Whether parameter `i` is a null form: a condition's form parameter stored as
    /// FormID 0 (`crate::cond`).
    pub fn is_null_form(&self, i: usize) -> bool {
        matches!(self.0.get(i), Some(ArgValue::Value(Value::Ref(0))))
    }

    /// A form; none for an absent argument or an empty reference (a value that is no
    /// form is the script's state, not a misread). Other kinds are recorded as unread.
    pub fn opt_form(&self, i: usize) -> Option<FormId> {
        match self.0.get(i)? {
            ArgValue::Value(v) => v.as_form().map(FormId),
            _ => {
                self.unread(i, "form");
                None
            }
        }
    }

    /// Parameter `i` as a form, without recording anything: for a parameter that takes
    /// another kind as well, read next (GetVATSValue's parameter).
    pub fn peek_form(&self, i: usize) -> Option<FormId> {
        match self.0.get(i)? {
            ArgValue::Value(v) => v.as_form().map(FormId),
            _ => None,
        }
    }

    /// Parameter `i` as a string, without recording anything: for a parameter that
    /// takes another kind as well, read next (GetIsUsedItemType's name or code).
    pub fn peek_str(&self, i: usize) -> Option<&str> {
        match self.0.get(i)? {
            ArgValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self, i: usize) -> Result<f64, String> {
        match self.0.get(i) {
            Some(a) => a.value().map(Value::num).ok_or_else(|| {
                self.unread(i, "number");
                format!("parameter {} is missing", i + 1)
            }),
            None => Err(format!("parameter {} is missing", i + 1)),
        }
    }

    pub fn num_or(&self, i: usize, d: f64) -> f64 {
        self.num(i).unwrap_or(d)
    }

    pub fn int(&self, i: usize) -> Result<i32, String> {
        self.num(i).map(|x| x as i32)
    }

    /// An enumeration code (actor value, sex, …), also accepted as a number.
    pub fn code(&self, i: usize) -> Result<u16, String> {
        match self.0.get(i) {
            Some(ArgValue::Enum(c)) => Ok(*c),
            Some(ArgValue::Value(v)) => Ok(v.num() as u16),
            _ => {
                self.unread(i, "code");
                Err(format!("parameter {} is not a code", i + 1))
            }
        }
    }

    pub fn axis(&self, i: usize) -> Result<usize, String> {
        let c = match self.0.get(i) {
            Some(ArgValue::Axis(c)) => *c,
            Some(ArgValue::Enum(c)) => char::from_u32(*c as u32).unwrap_or('?'),
            Some(ArgValue::Value(v)) => char::from_u32(v.num() as u32).unwrap_or('?'),
            _ => '?',
        };
        match c.to_ascii_uppercase() {
            'X' => Ok(0),
            'Y' => Ok(1),
            'Z' => Ok(2),
            _ => {
                self.unread(i, "axis");
                Err(format!("parameter {} is not an axis", i + 1))
            }
        }
    }

    pub fn str(&self, i: usize) -> Result<&str, String> {
        match self.0.get(i) {
            Some(ArgValue::Str(s)) => Ok(s),
            _ => {
                self.unread(i, "string");
                Err(format!("parameter {} is not a string", i + 1))
            }
        }
    }
}

pub type FnImpl = fn(&mut Game, &Ctx, &Args) -> Result<Value, String>;

#[derive(Clone, Copy)]
pub enum Kind {
    Impl(FnImpl),
    /// A system not built yet; the text names it. Recorded in the trace, no effect.
    Stub(&'static str),
    /// Deliberately nothing to do on this engine; the text says why. Returns 0.
    NoOp(&'static str),
}

/// Every registered function: opcode, catalogue name, kind.
pub fn registry() -> &'static [(u16, &'static str, Kind)] {
    // One static, so every call gives the same address (lookup and lookup_name hand out
    // references into it that are compared by pointer).
    static REGISTRY: &[(u16, &str, Kind)] = {
        use Kind::*;
        &[
            // Quests.
            (0x1039, "SetStage", Impl(quest::set_stage)),
            (0x103A, "GetStage", Impl(quest::get_stage)),
            (0x103B, "GetStageDone", Impl(quest::get_stage_done)),
            (0x1036, "StartQuest", Impl(quest::start_quest)),
            (0x1037, "StopQuest", Impl(quest::stop_quest)),
            (0x1038, "GetQuestRunning", Impl(quest::get_quest_running)),
            (0x1222, "GetQuestCompleted", Impl(quest::get_quest_completed)),
            (0x11A3, "SetObjectiveDisplayed", Impl(quest::set_objective_displayed)),
            (0x11A2, "SetObjectiveCompleted", Impl(quest::set_objective_completed)),
            (0x11A5, "GetObjectiveDisplayed", Impl(quest::get_objective_displayed)),
            (0x11A4, "GetObjectiveCompleted", Impl(quest::get_objective_completed)),
            (0x104F, "GetQuestVariable", Impl(quest::get_quest_variable)),
            (0x1035, "GetScriptVariable", Impl(quest::get_script_variable)),
            (0x11DC, "SetQuestDelay", Impl(quest::set_quest_delay)),
            (0x1071, "CompleteQuest", Impl(quest::complete_quest)),
            // References.
            (0x1021, "Enable", Impl(refs::enable)),
            (0x1022, "Disable", Impl(refs::disable)),
            (0x1023, "GetDisabled", Impl(refs::get_disabled)),
            (0x109E, "MoveTo", Impl(refs::move_to)),
            (0x124F, "MoveToMarkerWithFade", Impl(refs::move_to_marker_with_fade)),
            (0x10CE, "GetSelf", Impl(refs::get_self)),
            (0x1048, "GetIsID", Impl(refs::get_is_id)),
            (0x1088, "GetIsReference", Impl(refs::get_is_reference)),
            (0x1043, "GetInCell", Impl(refs::get_in_cell)),
            (0x1136, "GetInWorldspace", Impl(refs::get_in_worldspace)),
            (0x1020, "GetInSameCell", Impl(refs::get_in_same_cell)),
            (0x11BE, "GetInZone", Impl(refs::get_in_zone)),
            (0x112C, "IsInInterior", Impl(refs::is_in_interior)),
            (0x1001, "GetDistance", Impl(refs::get_distance)),
            (0x1006, "GetPos", Impl(refs::get_pos)),
            (0x1007, "SetPos", Impl(refs::set_pos)),
            (0x1008, "GetAngle", Impl(refs::get_angle)),
            (0x1009, "SetAngle", Impl(refs::set_angle)),
            (0x100A, "GetStartingPos", Impl(refs::get_starting_pos)),
            (0x100B, "GetStartingAngle", Impl(refs::get_starting_angle)),
            (0x116B, "GetLinkedRef", Impl(refs::get_linked_ref)),
            (0x1113, "GetParentRef", Impl(refs::get_parent_ref)),
            (0x11BB, "MarkForDelete", Impl(refs::mark_for_delete)),
            (0x10CC, "SetDestroyed", Impl(refs::set_destroyed)),
            (0x10CB, "GetDestroyed", Impl(refs::get_destroyed)),
            (0x1072, "Lock", Impl(refs::lock)),
            (0x1073, "Unlock", Impl(refs::unlock)),
            (0x1005, "GetLocked", Impl(refs::get_locked)),
            (0x100D, "Activate", Impl(refs::activate)),
            (0x10CD, "GetActionRef", Impl(refs::get_action_ref)),
            (0x1069, "IsActionRef", Impl(refs::is_action_ref)),
            (0x1161, "IsActor", Impl(refs::is_actor)),
            (0x1040, "GetIsCreature", Impl(refs::get_is_creature)),
            (0x1174, "IsInList", Impl(refs::is_in_list)),
            (0x1233, "AddFormToFormList", Impl(refs::add_form_to_form_list)),
            (0x10A5, "RemoveMe", Impl(items::remove_me)),
            // Items.
            (0x1002, "AddItem", Impl(items::add_item)),
            (0x1052, "RemoveItem", Impl(items::remove_item)),
            (0x102F, "GetItemCount", Impl(items::get_item_count)),
            (0x10AD, "RemoveAllItems", Impl(items::remove_all_items)),
            (0x10EE, "EquipItem", Impl(items::equip_item)),
            (0x10EF, "UnequipItem", Impl(items::unequip_item)),
            (0x10B6, "GetEquipped", Impl(items::get_equipped)),
            (0x11F4, "GetWeaponHealthPerc", Impl(items::get_weapon_health_perc)),
            // Actors.
            (0x100E, "GetActorValue", Impl(actor::get_actor_value)),
            (0x100F, "SetActorValue", Impl(actor::set_actor_value_watched)),
            (0x1010, "ModActorValue", Impl(actor::mod_actor_value_watched)),
            (0x110E, "ForceActorValue", Impl(actor::force_actor_value_watched)),
            (0x1181, "DamageActorValue", Impl(actor::damage_actor_value_watched)),
            (0x1182, "RestoreActorValue", Impl(actor::restore_actor_value_watched)),
            (0x1115, "GetBaseActorValue", Impl(actor::get_base_actor_value)),
            (0x11EF, "GetPermanentActorValue", Impl(actor::get_permanent_actor_value)),
            (0x102E, "GetDead", Impl(actor::get_dead)),
            (0x108B, "KillActor", Impl(actor::kill_actor)),
            (0x108C, "ResurrectActor", Impl(actor::resurrect_actor)),
            (0x1054, "GetDeadCount", Impl(actor::get_dead_count)),
            (0x1047, "GetInFaction", Impl(actor::get_in_faction)),
            (0x117F, "AddToFaction", Impl(actor::add_to_faction)),
            (0x1180, "RemoveFromFaction", Impl(actor::remove_from_faction)),
            (0x1049, "GetFactionRank", Impl(actor::get_faction_rank)),
            (0x1089, "SetFactionRank", Impl(actor::add_to_faction)),
            (0x11AB, "GetIsVoiceType", Impl(actor::get_is_voice_type)),
            (0x1046, "GetIsSex", Impl(actor::get_is_sex)),
            (0x102C, "SameSex", Impl(actor::same_sex)),
            (0x1087, "SameSexAsPC", Impl(actor::same_sex_as_pc)),
            (0x102A, "SameFaction", Impl(actor::same_faction)),
            (0x1193, "HasFriendDisposition", Impl(actor::has_friend_disposition)),
            (0x1080, "GetFatiguePercentage", Impl(actor::get_fatigue_percentage)),
            (0x1169, "GetTimeDead", Impl(actor::get_time_dead)),
            (0x11FE, "GetLastHitCritical", Impl(actor::get_last_hit_critical)),
            (0x11B1, "GetIsObjectType", Impl(actor::get_is_object_type)),
            (0x1028, "GetVampire", NoOp("a leftover from Oblivion that never returns anything but 0 [documented: GECK wiki GetVampire]")),
            (0x1083, "GetPCIsSex", Impl(actor::get_pc_is_sex)),
            (0x11E7, "SexChange", Impl(actor::sex_change)),
            (0x1045, "GetIsRace", Impl(actor::get_is_race)),
            (0x1044, "GetIsClass", Impl(actor::get_is_class)),
            (0x10EC, "SetGhost", Impl(actor::set_ghost)),
            (0x10F3, "SetRestrained", Impl(actor::set_restrained)),
            (0x10F4, "GetRestrained", Impl(actor::get_restrained)),
            (0x10F1, "SetUnconscious", Impl(actor::set_unconscious)),
            (0x10F2, "GetUnconscious", Impl(actor::get_unconscious)),
            (0x1150, "ResetHealth", Impl(actor::reset_health)),
            // The player.
            (0x1177, "RewardXP", Impl(player::reward_xp)),
            (0x11C1, "HasPerk", Impl(player::has_perk)),
            (0x1176, "AddPerk", Impl(player::add_perk)),
            (0x11A8, "RemovePerk", Impl(player::remove_perk)),
            (0x117E, "GetHasNote", Impl(player::get_has_note)),
            (0x117C, "AddNote", Impl(player::add_note)),
            (0x124A, "IsHardcore", Impl(player::is_hardcore)),
            (0x11CE, "IsPlayerTagSkill", Impl(player::is_player_tag_skill)),
            (0x11CD, "SetPlayerTagSkill", Impl(player::set_player_tag_skill)),
            (0x114A, "AddAchievement", Impl(player::add_achievement)),
            (0x1061, "DisablePlayerControls", Impl(player::disable_player_controls)),
            (0x1060, "EnablePlayerControls", Impl(player::enable_player_controls)),
            (0x1062, "GetPlayerControlsDisabled", Impl(player::get_player_controls_disabled)),
            (0x1050, "GetLevel", Impl(player::get_level)),
            (0x1138, "GetPCMiscStat", Impl(player::get_pc_misc_stat)),
            (0x1137, "ModPCMiscStat", Impl(player::mod_pc_misc_stat)),
            (0x11AF, "GetHealthPercentage", Impl(actor::get_health_percentage)),
            (0x117D, "RemoveNote", Impl(player::remove_note)),
            (0x126D, "ResetXP", Impl(player::reset_xp)),
            (0x123D, "GetReputation", Impl(player::get_reputation)),
            (0x1238, "SetReputation", Impl(player::set_reputation)),
            (0x1204, "RewardKarma", Impl(crate::crime::reward_karma)),
            (0x124B, "SetHardcore", Impl(player::set_hardcore)),
            (0x1055, "ShowMap", Impl(player::show_map)),
            (0x11EC, "GetMapMarkerVisible", Impl(player::get_map_marker_visible)),
            (0x1024, "MenuMode", Impl(misc::menu_mode)),
            (0x10DD, "SetOpenState", Impl(refs::set_open_state)),
            (0x109D, "GetOpenState", Impl(refs::get_open_state)),
            (0x10D7, "GetDefaultOpen", Impl(refs::get_default_open)),
            (0x11BA, "EnterTrigger", Impl(refs::enter_trigger)),
            (0x11C5, "SetPlayerTeammate", Impl(actor::set_player_teammate)),
            (0x11C6, "GetPlayerTeammate", Impl(actor::get_player_teammate)),
            // Messages, time, globals, numbers.
            (0x1059, "ShowMessage", Impl(misc::show_message)),
            (0x101F, "GetButtonPressed", Impl(misc::get_button_pressed)),
            (0x11B9, "ShowWarning", Impl(misc::show_warning)),
            (0x104D, "GetRandomPercent", Impl(misc::get_random_percent)),
            (0x100C, "GetSecondsPassed", Impl(misc::get_seconds_passed)),
            (0x104A, "GetGlobalValue", Impl(misc::get_global_value)),
            (0x1012, "GetCurrentTime", Impl(misc::get_current_time)),
            (0x1100, "GetGameSetting", Impl(misc::get_game_setting)),
            (0x1221, "Abs", Impl(misc::abs)),
            // State that only Layer 6 changes; with no Layer 6 it reads "no".
            (0x1121, "IsInCombat", Impl(crate::awareness::is_in_combat)),
            // The PC version [decision]: not a console.
            (0x1135, "IsXBox", Impl(misc::zero)),
            (0x120B, "IsPS3", Impl(misc::zero)),
            (0x108D, "IsTalking", Impl(misc::is_talking)),
            // Reputations (reputation.rs).
            (0x123F, "GetReputationThreshold", Impl(reputation::get_reputation_threshold)),
            (0x1239, "AddReputation", Impl(reputation::add_reputation)),
            (0x123A, "RemoveReputation", Impl(reputation::remove_reputation)),
            (0x123B, "AddReputationExact", Impl(reputation::add_reputation_exact)),
            (0x123E, "GetReputationPct", Impl(reputation::get_reputation_pct)),
            // Objects in the world (objects.rs).
            (0x1025, "PlaceAtMe", Impl(objects::place_at_me)),
            (0x11D1, "PlaceLeveledActorAtMe", Impl(objects::place_leveled_actor_at_me)),
            (0x10A6, "DropMe", Impl(objects::drop_me)),
            (0x1057, "Drop", Impl(objects::drop)),
            (0x1117, "SetOwnership", Impl(objects::set_ownership)),
            (0x118C, "ClearOwnership", Impl(objects::clear_ownership)),
            (0x1116, "IsOwner", Impl(objects::is_owner)),
            (0x1119, "SetCellOwnership", Impl(objects::set_cell_ownership)),
            (0x10BB, "SetCellPublicFlag", Impl(objects::set_cell_public_flag)),
            (0x116C, "DamageObject", Impl(objects::damage_object)),
            (0x11D7, "GetDestructionStage", Impl(objects::get_destruction_stage)),
            (0x11D8, "ClearDestruction", Impl(objects::clear_destruction)),
            (0x1260, "IsPlayerInRegion", Impl(objects::is_player_in_region)),
            (0x1264, "PlayerInRegion", Impl(objects::is_player_in_region)),
            (0x1063, "GetHeadingAngle", Impl(objects::get_heading_angle)),
            (0x122E, "HasLoaded3D", Impl(objects::has_loaded_3d)),
            (0x1018, "GetScale", Impl(objects::get_scale)),
            (0x113C, "SetScale", Impl(objects::set_scale)),
            (0x1041, "GetLockLevel", Impl(objects::get_lock_level)),
            (0x10E6, "GetInCellParam", Impl(objects::get_in_cell_param)),
            (0x10CF, "GetContainer", Impl(objects::get_container)),
            (0x1228, "SetMinimalUse", Impl(objects::set_minimal_use)),
            (0x1212, "SetCriticalStage", Impl(objects::set_critical_stage)),
            (0x1213, "IsInCriticalStage", Impl(objects::is_in_critical_stage)),
            (0x11D5, "SetBroadcastState", Impl(objects::set_broadcast_state)),
            (0x11D4, "GetBroadcastState", Impl(crate::radio::get_broadcast_state)),
            (0x1171, "SetTalkingActivatorActor", Impl(objects::set_talking_activator_actor)),
            (0x1172, "IsTalkingActivatorActor", Impl(crate::talking_activators::is_talking_activator_actor)),
            // Inventories (items.rs).
            (0x1249, "RemoveAllTypedItems", Impl(items::remove_all_typed_items)),
            (0x11BC, "AddItemHealthPercent", Impl(items::add_item_health_percent)),
            (0x11F6, "ModWeaponHealthPerc", Impl(items::mod_weapon_health_perc)),
            (0x11F5, "SetWeaponHealthPerc", Impl(items::set_weapon_health_perc)),
            (0x118F, "IsWeaponInList", Impl(items::is_weapon_in_list)),
            (0x106D, "IsWeaponSkillType", Impl(items::is_weapon_skill_type)),
            (0x11ED, "ResetInventory", Impl(items::reset_inventory)),
            (0x127A, "GetContainerInventoryCount", Impl(items::get_container_inventory_count)),
            (0x1155, "SetItemValue", Impl(items::set_item_value)),
            (0x110D, "SetQuestObject", Impl(items::set_quest_object)),
            // Magic effects and effect scripts (magic.rs).
            (0x101C, "AddSpell", Impl(magic::add_spell)),
            (0x101D, "RemoveSpell", Impl(magic::remove_spell)),
            (0x11D9, "CastImmediateOnSelf", Impl(magic::cast_immediate_on_self)),
            (0x1142, "Dispel", Impl(magic::dispel)),
            (0x1148, "DispelAllSpells", Impl(magic::dispel_all_spells)),
            (0x10DF, "IsSpellTarget", Impl(magic::is_spell_target)),
            (0x10D6, "HasMagicEffect", Impl(magic::has_magic_effect)),
            (0x122B, "GetSpellUsageNum", Impl(magic::get_spell_usage_num)),
            (0x10BA, "ScriptEffectElapsedSeconds", Impl(magic::script_effect_elapsed_seconds)),
            // Actors and factions.
            (0x116D, "IsChild", Impl(actor::is_child)),
            (0x11B6, "GetIsCreatureType", Impl(actor::get_is_creature_type)),
            (0x1003, "SetEssential", Impl(actor::set_essential)),
            (0x1273, "SetActorRefEssential", Impl(actor::set_actor_ref_essential)),
            (0x1162, "IsEssential", Impl(actor::is_essential)),
            (0x1272, "IsActorRefEssential", Impl(actor::is_essential)),
            (0x111C, "SetActorFullName", Impl(actor::set_actor_full_name)),
            (0x1199, "IsKiller", Impl(actor::is_killer)),
            (0x11C2, "GetFactionRelation", Impl(actor::get_faction_relation)),
            (0x119B, "GetFactionCombatReaction", Impl(actor::get_faction_combat_reaction)),
            (0x1078, "SetEnemy", Impl(actor::set_enemy)),
            (0x1079, "SetAlly", Impl(actor::set_ally)),
            (0x1214, "RemoveFromAllFactions", Impl(actor::remove_from_all_factions)),
            (0x103C, "GetFactionRankDifference", Impl(actor::get_faction_rank_difference)),
            (0x10C6, "SetPCEnemyOfFaction", Impl(actor::set_pc_enemy_of_faction)),
            (0x10C5, "GetPCEnemyOfFaction", Impl(actor::get_pc_enemy_of_faction)),
            (0x10C1, "GetPCExpelled", Impl(actor::get_pc_expelled)),
            (0x11C9, "ClearFactionPlayerEnemyFlag", Impl(actor::clear_faction_player_enemy_flag)),
            (0x11CC, "GetActorFactionPlayerEnemy", Impl(actor::get_actor_faction_player_enemy)),
            (0x10AB, "IgnoreCrime", Impl(actor::ignore_crime)),
            (0x10C0, "GetIgnoreCrime", Impl(actor::get_ignore_crime)),
            (0x1151, "SetIgnoreFriendlyHits", Impl(actor::set_ignore_friendly_hits)),
            (0x1152, "GetIgnoreFriendlyHits", Impl(actor::get_ignore_friendly_hits)),
            (0x10ED, "GetIsGhost", Impl(actor::get_ghost)),
            (0x1133, "SetLevel", Impl(actor::set_level)),
            (0x11C7, "GetPlayerTeammateCount", Impl(actor::get_player_teammate_count)),
            // The player (player.rs).
            (0x1111, "EnableFastTravel", Impl(player::enable_fast_travel)),
            (0x125E, "UnlockChallenge", Impl(player::unlock_challenge)),
            (0x1266, "GetChallengeCompleted", Impl(player::get_challenge_completed)),
            (0x126C, "RemoveRecurringFromChallenge", Impl(player::remove_recurring_from_challenge)),
            (0x10E7, "SetInCharGen", Impl(player::set_in_chargen)),
            (0x127F, "GetInCharGen", Impl(player::get_in_chargen)),
            (0x125C, "GetCasinoWinningsLevel", Impl(player::get_casino_winnings_level)),
            (0x1262, "GetCasinoWinningStage", Impl(player::get_casino_winnings_level)),
            (0x1229, "SetPCCanUsePowerArmor", Impl(player::set_pc_can_use_power_armor)),
            (0x1058, "AddTopic", Impl(player::add_topic)),
            (0x126B, "IsAlwaysHardcore", Impl(player::is_always_hardcore)),
            // Quests.
            (0x11AD, "CompleteAllObjectives", Impl(quest::complete_all_objectives)),
            (0x11DB, "ResetQuest", Impl(quest::reset_quest)),
            // Time and numbers.
            (0x10AA, "GetDayOfWeek", Impl(misc::get_day_of_week)),
            (0x121F, "Sqrt", Impl(misc::sqrt)),
            (0x120C, "IsWin32", Impl(misc::one)),
            // Gore is on, the game's default [decision].
            (0x1226, "IsGoreDisabled", Impl(misc::zero)),
            // Layer 6 systems: recorded, no effect.
            (0x105E, "EvaluatePackage", Impl(actor::evaluate_package)),
            (0x1097, "AddScriptPackage", Impl(actor::add_script_package)),
            (0x1098, "RemoveScriptPackage", Impl(actor::remove_script_package)),
            (0x11FA, "ResetAI", Impl(ai_state::reset_ai)),
            (0x1034, "SayTo", Impl(misc::say_to)),
            (0x1033, "Say", Impl(misc::say)),
            (0x1056, "StartConversation", Impl(misc::start_conversation)),
            (0x1016, "StartCombat", Impl(crate::awareness::start_combat)),
            (0x1017, "StopCombat", Impl(crate::awareness::stop_combat)),
            (0x1184, "SetDisposition", Impl(actor::set_disposition)),
            (0x115C, "SendTrespassAlarm", Impl(crate::crime::send_trespass_alarm)),
            (0x1013, "PlayGroup", Impl(crate::anims::play_group)),
            (0x1190, "PlayIdle", Impl(crate::idles::play_idle)),
            (0x105C, "Look", Impl(ai_state::look)),
            (0x105D, "StopLook", Impl(ai_state::stop_look)),
            (0x11E4, "AgeRace", Impl(actor::age_race)),
            (0x1026, "PlaySound", Impl(crate::sound::play_sound)),
            (0x11FF, "PlayMusic", Impl(crate::sound::play_music)),
            (0x1114, "PlayBink", Impl(misc::play_bink)),
            (0x1191, "ApplyImageSpaceModifier", Impl(misc::apply_image_space_modifier)),
            (0x1192, "RemoveImageSpaceModifier", Impl(misc::remove_image_space_modifier)),
            (0x1232, "IsImageSpaceActive", Impl(misc::is_image_space_active)),
            (0x1240, "TriggerScreenSplatter", Impl(misc::trigger_screen_splatter)),
            (0x1243, "SetScreenSplatterFade", Impl(misc::set_screen_splatter_fade)),
            (0x1241, "ClearScreenSplatter", Impl(misc::clear_screen_splatter)),
            (0x112D, "ForceWeather", Impl(crate::weather::force_weather)),
            (0x1158, "ReleaseWeatherOverride", Impl(crate::weather::release_weather_override)),
            (0x1186, "SetGlobalTimeMultiplier", Impl(misc::set_global_time_multiplier)),
            (0x1269, "SwapTexture", Impl(misc::swap_texture)),
            (0x1225, "AutoDisplayObjectives", Impl(ui::auto_display_objectives)),
            (0x11DD, "ForceActiveQuest", Impl(ui::force_active_quest)),
            (0x115E, "Autosave", Impl(ui::autosave)),
            (0x11E1, "GetPlayerName", Impl(misc::get_player_name)),
            (0x10DA, "ShowRaceMenu", Impl(misc::show_race_menu)),
            (0x121A, "SetTagSkills", Impl(misc::set_tag_skills)),
            (0x1268, "ShowTraitMenu", Impl(misc::show_trait_menu)),
            (0x10A0, "GetFurnitureMarkerID", Impl(furniture::get_furniture_marker_id)),
            (0x1187, "GetHitLocation", Impl(actor::get_hit_location)),
            (0x106E, "GetCurrentAIPackage", Impl(ai_state::get_current_ai_package)),
            (0x108F, "GetCurrentAIProcedure", Impl(ai_state::get_current_ai_procedure)),
            (0x10A1, "GetIsCurrentPackage", Impl(actor::get_is_current_package)),
            (0x10FF, "GetOffersServicesNow", Impl(ai_state::get_offers_services_now)),
            (0x10A2, "IsCurrentFurnitureRef", Impl(furniture::is_current_furniture_ref)),
            (0x1065, "IsWeaponOut", Impl(actor::is_weapon_out)),
            (0x106C, "GetWeaponAnimType", Impl(actor::get_weapon_anim_type)),
            (0x111E, "IsSneaking", Impl(senses::is_sneaking)),
            (0x1032, "GetTalkedToPC", Impl(misc::get_talked_to_pc)),
            (0x1245, "ShowLoveTesterMenuParams", Impl(misc::show_love_tester_menu)),
            (0x1173, "ShowBarterMenu", Impl(misc::show_barter_menu)),
            (0x1123, "PlayMagicShaderVisuals", Impl(misc::play_magic_shader_visuals)),
            (0x1129, "SetActorAlpha", Impl(misc::set_actor_alpha)),
            (0x10B2, "PlaySound3D", Impl(crate::sound::play_sound_3d)),
            (0x1198, "GetVATSValue", Impl(crate::vats::get_vats_value)),
            (0x105A, "SetAlert", Impl(crate::awareness::set_alert)),
            (0x1042, "GetShouldAttack", Impl(fighting::get_should_attack)),
            (0x1275, "IncrementScriptedChallenge", Impl(ui::increment_scripted_challenge)),
            (0x1201, "ResetPipboyManager", Impl(ui::reset_pipboy_manager)),
            (0x122C, "ForceRadioStationUpdate", Impl(crate::radio::force_radio_station_update)),
            // Layer 6 systems added 2026-10-09 (function coverage round): recorded, no effect.
            (0x1128, "IsAnimPlaying", Impl(crate::anims::is_anim_playing)),
            (0x10DB, "GetAnimAction", Impl(crate::anim_state::get_anim_action)),
            (0x1070, "IsIdlePlaying", Impl(crate::idles::is_idle_playing)),
            (0x1064, "PickIdle", Impl(crate::idles::pick_idle)),
            (0x111D, "IsLeftUp", Impl(actor::knocked_side_up)),
            (0x106A, "IsFacingUp", Impl(actor::knocked_side_up)),
            (0x1004, "Rotate", Impl(refs::rotate)),
            (0x109F, "GetSitting", Impl(furniture::get_sitting)),
            (0x10A3, "IsCurrentFurnitureObj", Impl(furniture::is_current_furniture_obj)),
            (0x1031, "GetSleeping", Impl(furniture::get_sleeping)),
            (0x105B, "GetIsAlerted", Impl(crate::awareness::get_is_alerted)),
            (0x10F5, "ForceFlee", Impl(fighting::force_flee)),
            (0x115B, "SetNoAvoidance", Impl(navigation::set_no_avoidance)),
            (0x118B, "SetActorsAI", Impl(ai_state::set_actors_ai)),
            (0x1122, "SetPackDuration", Impl(ai_state::set_pack_duration)),
            (0x10F6, "GetIsUsedItem", Impl(furniture::get_is_used_item)),
            (0x10F7, "GetIsUsedItemType", Impl(furniture::get_is_used_item_type)),
            (0x11E0, "GetIsUsedItemEquipType", Impl(furniture::get_is_used_item_equip_type)),
            (0x1103, "GetUsedItemActivate", Impl(furniture::get_used_item_activate)),
            (0x107E, "StartCannibal", Impl(actor::start_cannibal)),
            (0x10E3, "GetCannibal", Impl(actor::get_cannibal)),
            (0x107F, "HasBeenEaten", Impl(actor::has_been_eaten)),
            (0x107C, "StartMisterSandman", Impl(actor::start_mister_sandman)),
            (0x10E2, "GetSandman", Impl(actor::get_sandman)),
            (0x1101, "StopCombatAlarmOnActor", Impl(crate::awareness::stop_combat_alarm_on_actor)),
            (0x10B1, "SetCombatStyle", Impl(fighting::set_combat_style)),
            (0x11A0, "GetGroupMemberCount", Impl(fighting::get_group_member_count)),
            (0x11A1, "GetGroupTargetCount", Impl(fighting::get_group_target_count)),
            (0x10E8, "GetCombatTarget", Impl(crate::awareness::get_combat_target)),
            (0x1203, "IsCombatTarget", Impl(fighting::is_combat_target)),
            (0x11E2, "FireWeapon", Impl(fighting::fire_weapon)),
            (0x119C, "UseWeapon", Impl(crate::use_weapon::use_weapon)),
            (0x103F, "GetAttacked", Impl(crate::provocation::get_attacked)),
            (0x1120, "GetFriendHit", Impl(crate::provocation::get_friend_hit)),
            (0x106B, "GetKnockedState", Impl(fighting::get_knocked_state)),
            (0x11E9, "GetConcussed", Impl(fighting::get_concussed)),
            (0x118E, "IsLimbGone", Impl(fighting::is_limb_gone)),
            (0x11F0, "GetKillingBlowLimb", Impl(fighting::get_killing_blow_limb)),
            (0x118D, "GetCauseofDeath", Impl(fighting::get_cause_of_death)),
            (0x1259, "GetForceHitReaction", Impl(crate::anim_state::get_force_hit_reaction)),
            (0x1051, "GetArmorRating", Impl(fighting::get_armor_rating)),
            (0x11AC, "GetPlantedExplosive", Impl(actor::get_planted_explosive)),
            (0x124D, "DetonatePlacedExplosives", Impl(actor::detonate_placed_explosives)),
            (0x11EB, "SetVatsTarget", Impl(crate::vats::set_vats_target)),
            (0x10EB, "GetVATSTargetHeight", Impl(crate::vats::get_vats_target_height)),
            (0x10E0, "GetVATSMode", Impl(crate::vats::get_vats_mode)),
            (0x102D, "GetDetected", Impl(crate::detection::get_detected)),
            (0x10B4, "GetDetectionLevel", Impl(crate::detection::get_detection_level)),
            (0x101B, "GetLineOfSight", Impl(senses::get_line_of_sight)),
            (0x115A, "SetAllVisible", Impl(navigation::set_all_visible)),
            (0x1068, "CreateDetectionEvent", Impl(crate::awareness::create_detection_event)),
            // Karma and crime (crime.rs).
            (0x105F, "SendAssaultAlarm", Impl(crate::crime::send_assault_alarm)),
            (0x1090, "GetTrespassWarningLevel", Impl(crate::crime::get_trespass_warning_level)),
            (0x1091, "IsTrespassing", Impl(crate::crime::is_trespassing)),
            (0x103D, "GetAlarmed", Impl(crate::crime::get_alarmed)),
            (0x1075, "GetMajorCrimeCount", Impl(crate::crime::get_major_crime_count)),
            (0x1074, "GetMinorCrimeCount", Impl(crate::crime::get_minor_crime_count)),
            (0x11CB, "GetActorCrimePlayerEnemy", Impl(crate::crime::get_actor_crime_player_enemy)),
            (0x113A, "IsActorAVictim", Impl(crate::crime::is_actor_a_victim)),
            (0x10B0, "IsPCAMurderer", Impl(crate::crime::is_pc_a_murderer)),
            (0x11DA, "GetIsAlignment", Impl(crate::crime::get_is_alignment)),
            (0x1118, "IsCellOwner", Impl(crate::crime::is_cell_owner)),
            (0x1092, "IsInMyOwnedCell", Impl(crate::crime::is_in_my_owned_cell)),
            (0x124E, "GetOwnerLastTarget", Impl(actor::get_owner_last_target)),
            (0x11AE, "IsActorTalkingThroughActivator", Impl(crate::talking_activators::is_actor_talking_through_activator)),
            (0x11B3, "GetDialogueEmotion", Impl(misc::get_dialogue_emotion)),
            (0x104C, "GetDisposition", Impl(actor::get_disposition)),
            (0x1277, "AlwaysShowActorSubtitles", Impl(senses::always_show_actor_subtitles)),
            (0x1263, "PlaceAtReticle", Impl(objects::place_at_reticle)),
            (0x1019, "IsMoving", Impl(senses::is_moving)),
            (0x101A, "IsTurning", Impl(senses::is_turning)),
            (0x111F, "IsRunning", Impl(senses::is_running)),
            (0x10B9, "IsSwimming", Impl(senses::is_swimming)),
            (0x114C, "IsInDangerousWater", Impl(senses::is_in_dangerous_water)),
            (0x10D3, "SetForceSneak", Impl(senses::set_force_sneak)),
            (0x1166, "IsPlayerMovingIntoNewSpace", Impl(senses::is_player_moving_into_new_space)),
            (0x1170, "IsPlayerActionActive", Impl(crate::anim_state::is_player_action_active)),
            (0x1188, "IsPC1stPerson", Impl(senses::is_pc_1st_person)),
            (0x118A, "PushActorAway", Impl(fighting::push_actor_away)),
            (0x11D0, "IsPlayerGrabbedRef", Impl(senses::is_player_grabbed_ref)),
            (0x110C, "trapUpdate", Impl(crate::swing_traps::trap_update)),
            (0x11C0, "EnableNavMesh", Impl(navigation::enable_navmesh)),
            (0x11BF, "DisableNavmesh", Impl(navigation::disable_navmesh)),
            (0x1159, "setAllReachable", Impl(navigation::set_all_reachable)),
            (0x1125, "StopMagicShaderVisuals", Impl(misc::stop_magic_shader_visuals)),
            (0x1248, "SetSecuritronExpression", Impl(misc::set_securitron_expression)),
            (0x1146, "Reset3DState", Impl(misc::reset_3d_state)),
            (0x1211, "AttachAshPile", Impl(objects::attach_ash_pile)),
            (0x115D, "SetSceneIsComplex", NoOp("performance switch of the original's AI")),
            (0x1196, "SetCellImageSpace", Impl(misc::set_cell_image_space)),
            (0x1104, "SetWeather", Impl(crate::weather::set_weather)),
            (0x103E, "IsRaining", Impl(crate::weather::is_raining)),
            (0x104B, "IsSnowing", Impl(crate::weather::is_snowing)),
            (0x110A, "IsPleasant", Impl(crate::weather::is_pleasant)),
            (0x110B, "IsCloudy", Impl(crate::weather::is_cloudy)),
            (0x1094, "GetCurrentWeatherPercent", Impl(crate::weather::get_current_weather_percent)),
            (0x1095, "GetIsCurrentWeather", Impl(crate::weather::get_is_current_weather)),
            (0x11F7, "GetRadiationLevel", Impl(senses::get_radiation_level)),
            (0x11D6, "StartRadioConversation", Impl(crate::radio::start_radio_conversation)),
            (0x1224, "PipBoyRadioOff", Impl(crate::radio::pipboy_radio_off)),
            (0x11A7, "PipBoyRadio", Impl(crate::radio::pipboy_radio)),
            (0x11C4, "SetNPCRadio", Impl(crate::radio::set_npc_radio)),
            (0x11FB, "SetRumble", NoOp("controller rumble: no controller rumble on the PC version")),
            (0x1189, "PurgeCellBuffers", NoOp("memory: the original's cell buffers; cells here load and unload by distance")),
            (0x1163, "PreloadMagicEffect", NoOp("loading: effect particles load when drawn")),
            (0x1223, "ForceTerminalBack", Impl(ui::force_terminal_back)),
            // The console's camera (camera.rs; opcodes of our own, fnv_script::table::CONSOLE_OPCODE_BASE).
            (0xF04A, "ToggleFlyCam", Impl(camera::toggle_fly_cam)),
            (0xF033, "SetUFOCamSpeedMult", Impl(camera::set_ufo_cam_speed_mult)),
            (0xF02B, "SetCameraFOV", Impl(camera::set_camera_fov)),
            (0xF054, "ToggleMenus", Impl(camera::toggle_menus)),
            (0x125A, "ShowCaravanMenu", Impl(misc::show_caravan_menu)),
            (0x125B, "AddCardToPlayer", Impl(misc::add_card_to_player)),
            (0x11C8, "OpenTeammateContainer", Impl(crate::followers::open_teammate_container)),
            (0x1219, "AddSPECIALPoints", Impl(ui::add_special_points)),
            (0x1218, "SetSPECIALPoints", Impl(ui::set_special_points)),
            (0x11EE, "ShowSPECIALBookMenuParams", Impl(ui::show_special_book_menu)),
            (0x117A, "ShowRepairMenu", Impl(misc::show_repair_menu)),
            (0x1250, "ShowRecipeMenu", Impl(misc::show_recipe_menu)),
            (0x1254, "ShowBlackJackMenuParams", Impl(misc::show_blackjack_menu)),
            (0x1256, "ShowRouletteMenuParams", Impl(misc::show_roulette_menu)),
            (0x1252, "ShowSlotMachineMenuParams", Impl(misc::show_slot_machine_menu)),
            (0x11F1, "ShowBarberMenu", Impl(misc::show_barber_menu)),
            (0x11F2, "ShowPlasticSurgeonMenu", Impl(misc::show_plastic_surgeon_menu)),
            (0x11E3, "ShowTutorialMenu", Impl(ui::show_tutorial_menu)),
            (0x1279, "ShowSleepWaitMenu", Impl(crate::rest::show_sleep_wait_menu)),
            (0x1270, "EnableLoadingMenu", Impl(ui::enable_loading_menu)),
            (0x11F8, "ShowAllMapMarkers", Impl(player::show_all_map_markers)),
            (0x10AE, "WakeUpPC", Impl(crate::rest::wake_up_pc)),
            (0x10BD, "SetPCSleepHours", Impl(crate::rest::set_pc_sleep_hours)),
            (0x10AF, "IsPCSleeping", Impl(crate::rest::is_pc_sleeping)),
            (0x1109, "IsTimePassing", Impl(crate::rest::is_time_passing)),
            (0x120A, "GetIsLockBroken", Impl(refs::get_is_lock_broken)),
            (0x127B, "SystemSave", Impl(ui::system_save)),
            (0x1217, "ForceSave", Impl(ui::force_save)),
            (0x11B5, "ExitGame", Impl(ui::exit_game)),
            (0x121B, "AddTagSkills", Impl(ui::add_tag_skills)),
            (0x119F, "Exists", Impl(fighting::exists)),
            (0x1130, "IsWaterObject", Impl(senses::is_water_object)),
            // Used by the shipped scripts but unregistered before 2026-10-12 (script-stubs.md §6).
            (0x1236, "AddItemToLeveledList", Impl(items::add_item_to_leveled_list)),
            (0x126F, "IsDLCInstalled", Impl(misc::is_dlc_installed)),
            (0x1215, "GetXPForNextLevel", Impl(player::get_xp_for_next_level)),
            // Called only by Fallout 3's scripts (Tale of Two Wastelands; fo3.rs).
            (0x1053, "ModDisposition", Impl(fo3::mod_disposition)),
            (0x1082, "GetPCIsRace", Impl(fo3::get_pc_is_race)),
            (0x10D2, "GetForceSneak", Impl(fo3::get_force_sneak)),
            (0x11B7, "SetMerchantContainer", Impl(fo3::set_merchant_container)),
            (0x11E5, "MatchRace", Impl(fo3::match_race)),
            (0x11CA, "ClearActorsFactionsPlayerEnemyFlag", Impl(fo3::clear_actors_factions_player_enemy_flag)),
            (0x10BC, "GetPCSleepHours", Impl(fo3::get_pc_sleep_hours)),
            (0x11E6, "SetPCYoung", Impl(fo3::set_pc_young)),
            (0x1202, "SetPCToddler", Impl(fo3::set_pc_toddler)),
            (0x11FC, "SetNoActivationSound", Impl(fo3::set_no_activation_sound)),
            (0x11FD, "ClearNoActivationSound", Impl(fo3::clear_no_activation_sound)),
            (0x1200, "SetLocationSpecificLoadScreensOnly", Impl(fo3::set_location_specific_load_screens_only)),
            (0x1230, "SetLastExtDoorActivated", Impl(fo3::set_last_ext_door_activated)),
            (0x11DF, "MatchFaceGeometry", Impl(fo3::match_face_geometry)),
            (0x10B7, "Wait", Impl(fo3::wait)),
            (0x10B8, "StopWaiting", Impl(fo3::stop_waiting)),
            (0x106F, "IsWaiting", Impl(fo3::is_waiting)),
            (0x10A8, "SetFactionReaction", Impl(fo3::set_faction_reaction)),
            (0x111B, "SetCellFullName", Impl(fo3::set_cell_full_name)),
            (0x1127, "ResetInterior", Impl(fo3::reset_interior)),
            (0x11EA, "SetZoneRespawns", Impl(fo3::set_zone_respawns)),
            (0x122F, "DisableAllMines", Impl(fo3::disable_all_mines)),
            (0x1237, "RemapWaterType", Impl(fo3::remap_water_type)),
            (0x11F3, "TriggerLODApocalypse", Impl(fo3::trigger_lod_apocalypse)),
            (0x1144, "TriggerHitShader", Impl(fo3::trigger_hit_shader)),
            (0x1205, "TriggerScreenBlood", Impl(fo3::trigger_screen_blood)),
            (0x1227, "FadeSFX", Impl(fo3::fade_sfx)),
            (0x1231, "KillQuestUpdates", Impl(fo3::kill_quest_updates)),
            (0x116F, "GetLastPlayerAction", Impl(fo3::get_last_player_action)),
        ]
    };
    REGISTRY
}

/// Console functions the catalogue lists without an opcode, by catalogue name: the
/// function table gives them numbers of ours (`fnv_script::table::CONSOLE_OPCODE_BASE`),
/// which [`crate::game::Game::call_function`] dispatches here by name. A test checks
/// each name against the catalogue.
pub fn console_registry() -> &'static [(&'static str, Kind)] {
    use Kind::*;
    &[
        // World and render toggles (console.rs; the engine switches its own).
        ("ToggleCollision", Impl(toggles::toggle_collision)),
        ("ToggleActorsAI", Impl(toggles::toggle_actors_ai)),
        ("IsActorsAIOff", Impl(toggles::is_actors_ai_off)),
        ("ToggleDetection", Impl(toggles::toggle_detection)),
        ("ToggleGodMode", Impl(toggles::toggle_god_mode)),
        ("ToggleScripts", Impl(toggles::toggle_scripts)),
        ("ToggleGrass", Impl(toggles::toggle_grass)),
        ("ToggleSky", Impl(toggles::toggle_sky)),
        ("ToggleWaterSystem", Impl(toggles::toggle_water_system)),
        ("ToggleTrees", Impl(toggles::toggle_trees)),
        ("ToggleLeaves", Impl(toggles::toggle_leaves)),
        ("ToggleLODLand", Impl(toggles::toggle_lod_land)),
        ("ToggleNavmesh", Impl(toggles::toggle_navmesh)),
        ("ToggleLookIK", Impl(toggles::toggle_look_ik)),
        ("ToggleEmotions", Impl(toggles::toggle_emotions)),
        ("ToggleWireframe", Stub("rendering (wireframe: the renderer does not ask the GPU for its line mode)")),
    ]
}

/// The implementation registered for a console function by catalogue name (any case).
pub fn lookup_console(name: &str) -> Option<&'static Kind> {
    console_registry().iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, k)| k)
}

fn index() -> &'static HashMap<u16, usize> {
    static IX: OnceLock<HashMap<u16, usize>> = OnceLock::new();
    IX.get_or_init(|| registry().iter().enumerate().map(|(i, r)| (r.0, i)).collect())
}

/// The implementation registered for an opcode.
pub fn lookup(opcode: u16) -> Option<&'static Kind> {
    index().get(&opcode).map(|&i| &registry()[i].2)
}

/// Script-extender functions (catalogue `family: "extender"`), by catalogue name. The
/// game files give them no opcodes: a running extender assigns them (script-functions.md
/// §2), so they are dispatched by name, like [`console_registry`], once the extender
/// compiler and table give them numbers (docs/design/extender-compat.md). Empty until the
/// first one is built. A test checks each name against the catalogue.
pub fn ext_registry() -> &'static [(&'static str, Kind)] {
    &[]
}

fn name_index() -> &'static HashMap<String, &'static Kind> {
    static IX: OnceLock<HashMap<String, &'static Kind>> = OnceLock::new();
    IX.get_or_init(|| {
        let game = registry().iter().map(|(_, n, k)| (*n, k));
        let named = console_registry().iter().chain(ext_registry()).map(|(n, k)| (*n, k));
        game.chain(named).map(|(n, k)| (n.to_ascii_lowercase(), k)).collect()
    })
}

/// The implementation registered under a catalogue name (any case), in any of the three
/// registries: the game's opcodes, the console's and the extender's functions. Catalogue
/// names only; [`lookup_alias`] also takes aliases and observed spellings.
pub fn lookup_name(name: &str) -> Option<&'static Kind> {
    name_index().get(&name.to_ascii_lowercase()).copied()
}

/// As [`lookup_name`], and a name the function table knows (an alias such as `GetAV`, or
/// a spelling seen in shipped source) resolves to its function's entry.
pub fn lookup_alias(table: &fnv_script::table::FunctionTable, name: &str) -> Option<&'static Kind> {
    lookup_name(name).or_else(|| {
        let op = table.by_name(name)?;
        lookup(op).or_else(|| lookup_name(&table.get(op)?.name))
    })
}

/// Whether an opcode is implemented: an implementation, or deliberately nothing to do
/// (stubs count as handled, not implemented).
pub fn implemented(opcode: u16) -> bool {
    matches!(lookup(opcode), Some(Kind::Impl(_) | Kind::NoOp(_)))
}

pub fn is_stub(opcode: u16) -> bool {
    matches!(lookup(opcode), Some(Kind::Stub(_)))
}

pub fn yes(b: bool) -> Result<Value, String> {
    Ok(Value::bool(b))
}

pub fn none() -> Result<Value, String> {
    Ok(Value::ZERO)
}

/// The base form of a reference.
/// The destruction stage of a reference (−1 without data or damage).
pub(crate) fn objects_destruction_stage(g: &Game, r: FormId) -> i32 {
    objects::destruction_stage(g, r)
}

/// Disable a reference as `Disable` does.
pub(crate) fn objects_disable(g: &mut Game, r: FormId) {
    objects::disable_ref(g, r)
}

/// An actor's current health (Health's permanent part minus its damage).
pub fn actor_health(g: &mut Game, actor: FormId) -> Result<f64, String> {
    actor::actor_value(g, actor, crate::combat::HEALTH)
}

/// The base form of a reference (moved or created ones included; the player's base for
/// the player). A weapon's base form is its own base: a perk entry's Weapon tab runs its
/// conditions (IsWeaponSkillType, IsInList) on the weapon itself, not on a reference
/// (docs/spec-notes/perk-entry-points.md §3) [hypothesis].
pub fn base_of(g: &Game, r: FormId) -> Option<FormId> {
    if let Some((_, item)) = g.temp_item(r) {
        return Some(item);
    }
    match g.world.peek(r) {
        Some(s) => Some(s.base),
        None if r == FormId::PLAYER_REF => Some(FormId::PLAYER_BASE),
        // A created reference deleted before the last load keeps its base (#1002).
        None => g.world.placed.get(r).map(|p| p.base).or_else(|| g.world.deleted_base(r)).or_else(|| (g.data.sig(r) == Some(*b"WEAP")).then_some(r)),
    }
}

#[cfg(test)]
mod unread_tests {
    use super::{begin_unread, end_unread, Args, Unread};
    use fnv_script::vm::{ArgValue, Value};

    fn recorded(f: impl FnOnce()) -> Vec<Unread> {
        let outer = begin_unread();
        f();
        end_unread(outer)
    }

    #[test]
    fn a_present_argument_of_the_wrong_kind_is_recorded_and_an_absent_one_is_not() {
        // ShowMessage's compiled variables (#1117): one list, read as a number.
        let args = [ArgValue::Value(Value::Ref(0x38888)), ArgValue::Vars(vec![Value::Num(28.0)]), ArgValue::Int32(0)];
        let a = Args(&args);
        let u = recorded(|| {
            assert_eq!(a.num_or(1, 0.0), 0.0);
            assert_eq!(a.num_or(5, 0.0), 0.0); // absent: an optional parameter
            assert_eq!(a.num(2).ok(), Some(0.0)); // Int32 reads as a number
            assert_eq!(a.form(0).ok().map(|f| f.0), Some(0x38888));
        });
        assert_eq!(u, [Unread { param: 1, got: "message variables", wanted: "number" }]);
    }

    #[test]
    fn each_accessor_records_the_kinds_it_cannot_read() {
        let args = [ArgValue::Str("x".into()), ArgValue::Axis('Z'), ArgValue::Enum(3), ArgValue::Value(Value::Num(1.0))];
        let a = Args(&args);
        let u = recorded(|| {
            assert!(a.opt_form(1).is_none());
            assert!(a.code(0).is_err());
            assert!(a.axis(3).is_err());
            assert!(a.str(2).is_err());
            // Readable: a code as a number, an axis as an axis, a value as a form.
            assert!(a.num(2).is_ok() && a.axis(1).is_ok() && a.opt_form(3).is_some());
            assert!(a.str(0).is_ok());
            assert!(a.opt_form(9).is_none() && a.str(9).is_err()); // absent
        });
        let peeked = recorded(|| {
            assert!(a.peek_form(0).is_none() && a.peek_str(1).is_none());
            assert_eq!(a.peek_str(0), Some("x"));
            assert_eq!(a.peek_form(3).map(|f| f.0), Some(1));
        });
        assert_eq!(peeked, [], "a peek records nothing");
        let got: Vec<_> = u.iter().map(|u| (u.param, u.got, u.wanted)).collect();
        assert_eq!(got, [(1, "axis", "form"), (0, "string", "code"), (3, "number", "axis"), (2, "code", "string")]);
    }

    #[test]
    fn a_nested_call_keeps_its_own_record() {
        let args = [ArgValue::Str("x".into())];
        let a = Args(&args);
        let outer = begin_unread();
        let _ = a.num(0);
        let inner = recorded(|| {
            let _ = a.code(0);
        });
        let mine = end_unread(outer);
        assert_eq!(inner.iter().map(|u| u.wanted).collect::<Vec<_>>(), ["code"]);
        assert_eq!(mine.iter().map(|u| u.wanted).collect::<Vec<_>>(), ["number"]);
    }
}
