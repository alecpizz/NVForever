//! Inventory functions over the Layer 4 world, with the OnAdd / OnEquip / OnUnequip
//! events of scripted items.

use super::{none, yes, Args, Ctx};
use crate::game::{event, menus, Game, InstKey};
use crate::trace::EventKind;
use fnv_script::vm::Value;
use fnv_sim::forms::ItemKind;
use fnv_sim::inventory::Extra;
use fnv_sim::world::EquipLock;
use fnv_sim::FormId;

/// The items an item parameter stands for: the form itself, or every member of a form
/// list [documented: GECK wiki AddItem and RemoveItem notes, a form list applies the
/// count to each item in it].
fn items(g: &Game, item: FormId) -> Vec<FormId> {
    if g.data.sig(item).as_ref() == Some(b"FLST") {
        super::refs::list_members(g, item)
    } else {
        vec![item]
    }
}

/// A hide-message flag argument: non-zero hides the on-screen "added"/"removed" line
/// [documented: GECK wiki AddItem SilentlyAdd, RemoveItem HideMessageFlag,
/// AddItemHealthPercent MessageHiddenFlag, RemoveAllItems hideMessages].
fn hide_flag(a: &Args, i: usize) -> bool {
    a.num_or(i, 0.0) != 0.0
}

/// `Container.AddItem item count [silent]`: leveled lists resolve as in Layer 4; OnAdd
/// runs on the item's script [documented: GECK wiki AddItem, once per item in the count;
/// here once per call, hypothesis].
/// The condition AddItem gives a weapon or armour (see [`add_item`]).
pub const ADDED_CONDITION: f32 = 0.999;

pub fn add_item(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    // Muted while co-op replays claimed stages: given live already, or a scene for a
    // player who wasn't there (`crate::coop_joint`) [design].
    if g.coop_replaying && ctx.subject() == Ok(FormId::PLAYER_REF) {
        return none();
    }
    story_items(g, ctx, |g| add_item_now(g, ctx, a))
}

fn add_item_now(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let to = ctx.subject()?;
    let count = a.int(1)?;
    let silent = hide_flag(a, 2);
    for item in items(g, a.form(0)?) {
        // A weapon or armour given by AddItem comes at 99.9% of its health [observed: the
        // peer's jip-fifth rows, a 10mm pistol 79.92 of 80, leather armour 149.85 of 150].
        let added = match g.data.sig(item) {
            Some(s) if &s == b"WEAP" || &s == b"ARMO" => g.world.add_item_at_condition(to, item, count, Some(ADDED_CONDITION)),
            _ => g.world.add_item(to, item, count),
        }
        .map_err(|e| e.to_string())?;
        for (i, n) in added {
            g.emit(EventKind::ItemAdded { to, item: i, count: n, silent });
            g.item_event(to, i, event::ON_ADD, Some(Value::Ref(to.0)));
        }
    }
    review_equipment(g, to);
    mark_item_menu_refresh(g);
    none()
}

/// Request a one-shot menu refresh when `AddItem` or `AddItemHealthPercent` runs while
/// an inventory, container, or barter menu is open, keeping `MenuModel` aligned with
/// `World` [design; Stewie's Tweaks 9.70 public changelog].
fn mark_item_menu_refresh(g: &mut Game) {
    if g.menu_open(menus::INVENTORY) || g.menu_open(menus::CONTAINER) || g.menu_open(menus::BARTER) {
        g.gaps.jip3.refresh_items_list = true;
    }
}

/// An actor given something by AddItem equips the best armour and weapon it has
/// (`fnv_sim::World::review_equipment`) [documented: GECK wiki AddItem notes]: OnEquip and
/// OnUnequip run for what changed (owner's playtest list 7 item 12: Trudy is given a
/// shotgun by VMS16 stage 70 and fought unarmed).
pub fn review_equipment(g: &mut Game, actor: FormId) {
    let changed = g.world.review_equipment(actor);
    if changed.is_empty() {
        return;
    }
    for (item, off) in changed {
        tracing::info!("equipment review: {} {actor} equips {} {item} (given an item){}", g.data.name(actor), g.data.name(item), if off.is_empty() { String::new() } else { format!(" in place of {}", off.iter().map(|o| g.data.name(*o)).collect::<Vec<_>>().join(", ")) });
        for u in off {
            g.item_event(actor, u, event::ON_UNEQUIP, Some(Value::Ref(actor.0)));
        }
        g.equip_events(actor, item, false);
    }
    g.sync_actor_effects(actor);
}

/// `Container.RemoveItem item count [hideMessage]`: the items are destroyed. The
/// opening's `player.RemoveItem` of the Pip-Boy 3000 and its glove passes 1 [observed:
/// FalloutNV.esm scripts, decompiled]. A count of 0 removes nothing and the script goes
/// on [observed: observer check 44, capture o5_00: `player.RemoveItem` of Stimpaks and
/// caps with 0 left both counts as they were and the next line ran]; Loyal's reward line
/// (INFO 0x0010A1BF) passes one when Nellis' arrays are not all fixed (#1113).
pub fn remove_item(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    story_items(g, ctx, |g| remove_item_now(g, ctx, a))
}

fn remove_item_now(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let from = ctx.subject()?;
    let count = a.int(1)?;
    if count == 0 {
        return none();
    }
    let silent = hide_flag(a, 2);
    for item in items(g, a.form(0)?) {
        let n = g.world.remove_item(from, item, count).map_err(|e| e.to_string())?;
        if n > 0 {
            g.emit(EventKind::ItemRemoved { from, item, count: n, silent });
        }
    }
    none()
}

/// `Container.GetItemCount item`: how many are carried; for a form list, the sum over its
/// members [documented: GECK wiki GetItemCount notes]. Notes do not count ("does not
/// work with Notes" [documented: same notes]; observed 0 for a holotape just added,
/// capture `fa_batch3_actions`).
pub fn get_item_count(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let of = ctx.subject()?;
    let mut n = 0;
    for item in items(g, a.form(0)?) {
        if g.data.sig(item).as_ref() == Some(b"NOTE") {
            continue;
        }
        n += g.world.item_count(of, item).map_err(|e| e.to_string())?;
    }
    Ok(Value::Num(n as f64))
}

/// `RemoveAllItems [target] [retainOwnership] [hideMessages]`: into the target
/// container, or destroyed. Moved items keep their owner only with retainOwnership
/// non-zero [documented: GECK wiki RemoveAllItems] (`fnv_sim::World::transfer_all_owned`).
///
/// Moved items run their OnAdd in the target, once per stack as AddItem's once per call
/// [hypothesis: OnAdd "runs when the object is added to a container", GECK wiki OnAdd;
/// Ringo's Caravan Deck relies on it: its OnAdd moves the spare deck's 54 cards to the
/// player with RemoveAllItems and each card's OnAdd runs AddCardToPlayer, caravan.md §2].
pub fn remove_all_items(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let to_player = a.opt_form(0) == Some(FormId::PLAYER_REF);
    no_hand_out(g, to_player, |g| story_items(g, ctx, |g| remove_all_items_now(g, ctx, a)))
}

/// Co-op: a container that gives the player everything it holds (a checkpoint giving the
/// player's own items back: Dead Money's SCPT 0x01013FB4, a casino's door) hands the
/// guests nothing (`crate::coop_rewards`): what it gives was the player's (
/// the way out of the Sierra Madre handed the host's own items out as NVDLC01MQ03's)
/// [design].
fn no_hand_out(g: &mut Game, to_player: bool, f: impl FnOnce(&mut Game) -> Result<Value, String>) -> Result<Value, String> {
    let on = g.coop_rewards.on;
    if to_player {
        g.coop_rewards.on = false;
    }
    let r = f(g);
    g.coop_rewards.on = on;
    r
}

fn remove_all_items_now(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let from = ctx.subject()?;
    let silent = hide_flag(a, 2);
    let keep_owner = a.num_or(1, 0.0) != 0.0;
    let mut added = Vec::new();
    let stacks = match a.opt_form(0) {
        Some(to) => {
            if to == FormId::PLAYER_REF && !g.coop_guest && g.coop_checkpoint_holds.remove(&from) {
                g.coop_checkpoints.push((from, -1, None, true));
            }
            // Co-op: the player's whole gear put away (Dead Money's bunker trap, SCPT
            // 0x0100C59E `player.RemoveAllItems` into 0x0100EFCC): every guest
            // hands over its own the same, and gets it back when the container gives the
            // host's back (above) [design: the script names the player only; each player].
            if from == FormId::PLAYER_REF && to != FormId::PLAYER_REF && !g.coop_guest && g.coop_effects.is_some() {
                g.coop_checkpoints.push((to, -1, None, false));
                g.coop_checkpoint_holds.insert(to);
            }
            let moved = g.world.transfer_all_owned(from, to, keep_owner).map_err(|e| e.to_string())?;
            for &(item, count) in &moved {
                g.emit(EventKind::ItemAdded { to, item, count, silent });
                added.push((to, item));
            }
            moved
        }
        None => g.world.remove_all_items(from).map_err(|e| e.to_string())?,
    };
    for (item, count) in stacks {
        g.emit(EventKind::ItemRemoved { from, item, count, silent });
    }
    for (to, item) in added {
        g.item_event(to, item, event::ON_ADD, Some(Value::Ref(to.0)));
    }
    none()
}

/// `actor.EquipItem item [NoUnequipFlag] [HideEquipMessage]`. EquipItem takes away a
/// NoEquip lock UnequipItem put on the item; with NoUnequipFlag 1 the item is locked on:
/// neither the actor nor the player's inventory can take it off, and nothing else goes
/// into its slot, until UnequipItem [documented: GECK wiki EquipItem]
/// (`fnv_sim::EquipLock`). A slot held by a locked item refuses the item [documented:
/// same page], an error of the call.
pub fn equip_item(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let item = a.form(0)?;
    if g.handled(crate::nvse_showoff::ON_PRE_ACTIVATE_INVENTORY_ITEM_ALT)
        && g.world.item_count(actor, item).unwrap_or(0) > 0
        && !g.pre_activate_inventory_item_alt(actor, item, 0, true)
    {
        return none();
    }
    if g.world.equip_lock(actor, item) == Some(EquipLock::NoEquip) {
        g.world.lock_equip(actor, item, None).map_err(|e| e.to_string())?;
    }
    let e = match g.world.equip(actor, item) {
        // Power armour on the untrained player: the game's message, as from the Pip-Boy
        // [hypothesis: a script's EquipItem is refused as the player's own equipping is].
        Err(fnv_sim::world::SimError::NoPowerArmorTraining(_)) => {
            super::player::refuse_power_armor(g);
            return Err(format!("{} needs Power Armor Training", g.data.name(item)));
        }
        r => r.map_err(|e| e.to_string())?,
    };
    if !e.consumed && hide_flag(a, 1) {
        g.world.lock_equip(actor, item, Some(EquipLock::NoUnequip)).map_err(|e| e.to_string())?;
    }
    if !e.consumed && actor == FormId::PLAYER_REF {
        g.script_worn.insert(item, hide_flag(a, 1));
    }
    for u in e.unequipped {
        g.item_event(actor, u, event::ON_UNEQUIP, Some(Value::Ref(actor.0)));
    }
    // The actor's own OnActorEquip runs before the item counts as equipped, for an aid
    // item used up too (`Game::equip_events`).
    g.ref_event(actor, event::ON_ACTOR_EQUIP, Some(Value::Ref(item.0)), Some(actor));
    if e.consumed {
        // An aid item is consumed: it is used as from the inventory (Game::consume), with
        // potency and the addiction roll [documented: GECK wiki EquipItem, "the item is
        // treated as though it were used normally"].
        // The hide-equip-message flag also hides its "removed" line [hypothesis].
        g.emit(EventKind::ItemRemoved { from: actor, item, count: 1, silent: hide_flag(a, 2) });
        // A skill book is read (`crate::books`) [hypothesis: as from the inventory].
        if g.read_book(actor, item).is_none() {
            g.ingest(actor, item);
        }
    } else {
        g.item_event(actor, item, event::ON_EQUIP, Some(Value::Ref(actor.0)));
    }
    // A worn enchantment takes hold at once (`crate::abilities`).
    g.sync_actor_effects(actor);
    none()
}

/// `actor.UnequipItem item [NoEquipFlag] [HideEquipMessage]`. UnequipItem takes away a
/// NoUnequip lock EquipItem put on; with NoEquipFlag 1 the item is locked off until
/// EquipItem [documented: GECK wiki UnequipItem] (`fnv_sim::EquipLock`). The page's note
/// that companions ignore the flag is not modelled: a fighting NPC takes up its best weapon
/// again (`crate::style_weapon`), but not one locked off.
/// HideEquipMessage has nothing to hide: the engine shows no equip lines.
pub fn unequip_item(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let item = a.form(0)?;
    // Scripted UnequipItem dispatches ShowOff:OnPreActivateInventoryItemAlt (with
    // bEquipping = 1) and honors SetFunctionValue 0 veto [observed: 2026-11-24,
    // Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, GSDocMitchellHouse
    // (0x00103DF9), 10 official base masters,
    // `tools/capture/nvse/catalogs/wave6d1_pre_activate_inv_alt_unequip.json` probes
    // `w6d1_15_pre_activate_inv_aid_and_unequip` and
    // `w6d1p3_15_unequip_alt_beq_and_veto`: `pre=1|veto(eq=1,n=1,bEq=1)|alw(eq=0,n=2,bEq=1)`;
    // no DLL, PDB, or third-party source consulted].
    if g.handled(crate::nvse_showoff::ON_PRE_ACTIVATE_INVENTORY_ITEM_ALT)
        && g.world.get(actor).ok().and_then(|s| s.inventory.as_ref()).is_some_and(|i| i.is_equipped(item))
        && !g.pre_activate_inventory_item_alt(actor, item, 0, true)
    {
        return none();
    }
    if g.world.equip_lock(actor, item) == Some(EquipLock::NoUnequip) {
        g.world.lock_equip(actor, item, None).map_err(|e| e.to_string())?;
    }
    // xNVSE OnActorUnequip / OnUnequip fires PRE-unequip (`GetEquipped == 1`) with null
    // calling reference (`GetSelfAlt == 0`) [observed: `w6d1_08_on_actor_equip_unequip_aliases`].
    let was_equipped = g.world.get(actor).ok().and_then(|s| s.inventory.as_ref()).is_some_and(|i| i.is_equipped(item));
    if was_equipped {
        g.dispatch_event("onactorunequip", vec![fnv_script::vm::XVal::Ref(actor.0), fnv_script::vm::XVal::Ref(item.0)], None);
    }
    if g.world.unequip(actor, item).map_err(|e| e.to_string())? {
        g.item_block_only(actor, item, event::ON_UNEQUIP, Some(Value::Ref(actor.0)));
    }
    if actor == FormId::PLAYER_REF {
        g.script_worn.remove(&item);
    }
    if hide_flag(a, 1) {
        g.world.lock_equip(actor, item, Some(EquipLock::NoEquip)).map_err(|e| e.to_string())?;
    }
    g.sync_actor_effects(actor);
    none()
}

/// `actor.GetEquipped item`: 1 when the actor has the item equipped; for a form list,
/// when any member is [documented: GECK wiki GetEquipped notes]. An NPC has its outfit
/// and weapon equipped once it has been loaded (`fnv_sim::World::outfit`).
pub fn get_equipped(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let wanted = items(g, a.form(0)?);
    // A co-op guest's shot, run as the player's: the weapon it was fired with
    // is what the player has equipped as far as weapons go.
    if let Some(w) = g.shot_for_player.filter(|_| actor == FormId::PLAYER_REF) {
        if wanted.contains(&w) {
            return yes(true);
        }
        if wanted.iter().all(|&i| g.world.forms.item(i).is_some_and(|f| f.kind == fnv_sim::forms::ItemKind::Weapon)) {
            return yes(false);
        }
    }
    let s = g.world.get(actor).map_err(|e| e.to_string())?;
    yes(s.inventory.as_ref().is_some_and(|i| wanted.iter().any(|&w| i.is_equipped(w))))
}

/// The condition of the equipped weapon in percent [documented: GECK wiki
/// GetWeaponHealthPerc]: its stack's condition, 100 for an undamaged one or with no
/// weapon equipped [hypothesis for "no weapon": the capture saves never reached the
/// tutorial stage that fires below 25 although a new character starts unarmed,
/// layer5-captures.md §2 (CGTutorial stage 218 not done)].
pub fn get_weapon_health_perc(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let forms = g.world.forms;
    let s = g.world.get(actor).map_err(|e| e.to_string())?;
    let cond = s.inventory.as_ref().and_then(|inv| {
        inv.groups()
            .find(|(item, e, _)| e.equipped && forms.item(*item).is_some_and(|i| i.kind == fnv_sim::forms::ItemKind::Weapon))
            .map(|(_, e, _)| e.condition.unwrap_or(1.0))
    });
    Ok(Value::Num(cond.unwrap_or(1.0) as f64 * 100.0))
}

/// `RemoveMe [container]`: the calling inventory item leaves the container holding it
/// (one of the stack); with a container, a copy goes there. It acts like a return
/// [documented: catalogue summary and notes of RemoveMe]. The item's script instance is
/// dropped when the holder has none of the item left (a later copy starts fresh
/// [hypothesis: a copy does not keep the variables, catalogue notes]). No "removed" line
/// shows [documented: GECK wiki RemoveMe, "does not display the "xxx has been removed."
/// message"; 88 of the shipped calls are in `OnAdd player` blocks]; nor an "added" line
/// for the copy [hypothesis: one silent move; capture asked].
pub fn remove_me(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let Some(key @ InstKey::Item { holder, item }) = ctx.inst else {
        return Err("RemoveMe outside the script of an inventory item".into());
    };
    let n = g.world.remove_item(holder, item, 1).map_err(|e| e.to_string())?;
    if n > 0 {
        g.emit(EventKind::ItemRemoved { from: holder, item, count: n, silent: true });
    }
    if let Some(to) = a.opt_form(0) {
        for (i, c) in g.world.add_item(to, item, 1).map_err(|e| e.to_string())? {
            g.emit(EventKind::ItemAdded { to, item: i, count: c, silent: true });
        }
    }
    if g.world.item_count(holder, item).unwrap_or(0) == 0 {
        g.instances.remove(&key);
    }
    g.request_return();
    none()
}

impl Game<'_> {
    /// The weapon `actor` has equipped ([`equipped_weapon`]).
    pub fn weapon_equipped(&mut self, actor: FormId) -> Option<FormId> {
        equipped_weapon(self, actor)
    }
}

/// The weapon an actor has equipped, if any (including `TempCloneForm` weapons).
pub fn equipped_weapon(g: &mut Game, actor: FormId) -> Option<FormId> {
    let forms = g.world.forms;
    let clones = &g.clones;
    let s = g.world.get(actor).ok()?;
    let inv = s.inventory.as_ref()?;
    inv.equipped().into_iter().find(|&i| {
        let mut orig = i;
        for _ in 0..16 {
            let Some(&p) = clones.get(&orig) else { break };
            orig = p;
        }
        forms.item(orig).is_some_and(|it| it.kind == ItemKind::Weapon)
    })
}

/// Inventory form types by the codes RemoveAllTypedItems takes. 40 is the weapons
/// [documented: GECK wiki RemoveAllTypedItems example]; the other codes follow the
/// form type numbering published for this engine by the script extender community
/// [hypothesis: not checked against a source we can cite here; the shipped scripts pass
/// 24, 26, 31, 40, 41 and 47 together to empty an inventory, observed].
pub fn form_type_code(sig: &[u8; 4]) -> Option<i32> {
    Some(match sig {
        b"ARMO" => 24,
        b"BOOK" => 25,
        b"INGR" => 29,
        b"LIGH" => 30,
        b"MISC" => 31,
        b"WEAP" => 40,
        b"AMMO" => 41,
        b"KEYM" => 46,
        b"ALCH" => 47,
        b"NOTE" => 49,
        _ => return None,
    })
}

/// `container.RemoveAllTypedItems [target] [retainOwnership] [suppressMessages]
/// [typeCode] [exceptionList]` [documented: GECK wiki RemoveAllTypedItems]: the items
/// of the form type (all with no code or −1, none with 0) leave the container, except
/// non-playable biped items, members of the exception list and, from the player, quest
/// items. With a target they move there keeping their condition (and their owner with
/// the retain flag, otherwise the owner is cleared); without one they are destroyed.
pub fn remove_all_typed_items(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let to_player = a.opt_form(0) == Some(FormId::PLAYER_REF);
    no_hand_out(g, to_player, |g| story_items(g, ctx, |g| remove_all_typed_items_now(g, ctx, a)))
}

fn remove_all_typed_items_now(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let from = ctx.subject()?;
    let to = a.opt_form(0);
    let code = a.num(3).map(|c| c as i32).unwrap_or(-1);
    let except_list = a.opt_form(4);
    if code != 0 {
        // Co-op: the player's things put away by type (a checkpoint's container) are put
        // away for the guests too.
        if let Some(to) = to.filter(|_| from == FormId::PLAYER_REF && !g.coop_guest) {
            g.coop_checkpoints.push((to, code, except_list, false));
            g.coop_checkpoint_holds.insert(to);
        }
        // The checkpoint giving the player's things back by type, as the Fort's gate does
        // (`<container>.RemoveAllTypedItems player 1 1`, SCPT 0x00164763): the
        // guests get theirs back too.
        if to == Some(FormId::PLAYER_REF) && !g.coop_guest && g.coop_checkpoint_holds.remove(&from) {
            g.coop_checkpoints.push((from, -1, None, true));
        }
    }
    let except = except_list.map(|l| super::refs::list_members(g, l)).unwrap_or_default();
    let how = RemoveAll { to, retain: a.num_or(1, 0.0) != 0.0, silent: hide_flag(a, 2), code, except, quest_items: false, unplayable: false };
    remove_all(g, from, &how)?;
    none()
}

/// What RemoveAllTypedItems and ShowOff's RemoveAllItemsShowOff take: the target, the
/// owner kept, the messages hidden, the form type code (-1 any, 0 none), the exceptions,
/// and whether the player's quest items and non-playable biped items go too.
#[derive(Debug, Clone, Default)]
pub struct RemoveAll {
    pub to: Option<FormId>,
    pub retain: bool,
    pub silent: bool,
    pub code: i32,
    pub except: Vec<FormId>,
    pub quest_items: bool,
    pub unplayable: bool,
}

/// The items `how` picks leave `from` (module notes of [`remove_all_typed_items`]).
pub fn remove_all(g: &mut Game, from: FormId, how: &RemoveAll) -> Result<(), String> {
    let (to, retain, silent, code) = (how.to, how.retain, how.silent, how.code);
    if code == 0 {
        return Ok(());
    }
    let except = &how.except;
    let forms = g.world.forms;
    let stacks: Vec<(FormId, i32)> = {
        let s = g.world.get(from).map_err(|e| e.to_string())?;
        s.inventory.as_ref().map(|i| i.iter().collect()).unwrap_or_default()
    };
    for (item, n) in stacks {
        if g.world.resolve_item(item).is_none() {
            continue;
        }
        if (g.world.effective_non_playable_biped(item) && !how.unplayable) || except.contains(&item) {
            continue;
        }
        if from == FormId::PLAYER_REF && !how.quest_items && g.world.is_quest_item(item) {
            continue;
        }
        if code != -1 && forms.sig(g.unclone(item)).as_ref().and_then(form_type_code) != Some(code) {
            continue;
        }
        let groups = fnv_sim::inventory::unequipped(g.world.inventory_mut(from).map_err(|e| e.to_string())?.take(item, n));
        g.emit(EventKind::ItemRemoved { from, item, count: n, silent });
        if let Some(to) = to {
            for (mut extra, c) in groups {
                if !retain {
                    extra.owner = None;
                }
                g.world.add_item_extra(to, item, c, extra).map_err(|e| e.to_string())?;
            }
            g.emit(EventKind::ItemAdded { to, item, count: n, silent });
        }
    }
    Ok(())
}

/// `container.AddItemHealthPercent item count health [hidden]`: AddItem with the items
/// at that condition (a fraction, 0.85 for 85%) [documented: catalogue notes]. A
/// leveled list resolves as AddItem, its items at the given condition rather than the
/// list's own (`World::add_item_at_condition`).
pub fn add_item_health_percent(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    story_items(g, ctx, |g| add_item_health_percent_now(g, ctx, a))
}

fn add_item_health_percent_now(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let to = ctx.subject()?;
    let item = a.form(0)?;
    let count = a.int(1)?;
    let health = a.num(2)?.clamp(0.0, 1.0) as f32;
    if g.world.forms.leveled(item).is_some() {
        let silent = hide_flag(a, 3);
        let added = g.world.add_item_at_condition(to, item, count, (health < 1.0).then_some(health)).map_err(|e| e.to_string())?;
        for (i, n) in added {
            g.emit(EventKind::ItemAdded { to, item: i, count: n, silent });
            g.item_event(to, i, event::ON_ADD, Some(Value::Ref(to.0)));
        }
        mark_item_menu_refresh(g);
        return none();
    }
    let extra = Extra { condition: (health < 1.0).then_some(health), ..Extra::default() };
    g.world.add_item_extra(to, item, count, extra).map_err(|e| e.to_string())?;
    g.emit(EventKind::ItemAdded { to, item, count, silent: hide_flag(a, 3) });
    g.item_event(to, item, event::ON_ADD, Some(Value::Ref(to.0)));
    mark_item_menu_refresh(g);
    none()
}

fn set_weapon_condition(g: &mut Game, actor: FormId, f: impl Fn(f32) -> f32) -> Result<(), String> {
    let w = equipped_weapon(g, actor).ok_or("no weapon equipped")?;
    let inv = g.world.inventory_mut(actor).map_err(|e| e.to_string())?;
    inv.map_equipped(w, |e| {
        let c = f(e.condition.unwrap_or(1.0)).clamp(0.0, 1.0);
        Extra { condition: (c < 1.0).then_some(c), ..e }
    });
    Ok(())
}

/// `actor.ModWeaponHealthPerc amount`: the equipped weapon's condition changes by
/// `amount` percentage points [hypothesis: the unit of GetWeaponHealthPerc], within
/// 0–100.
pub fn mod_weapon_health_perc(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let d = a.num(0)? as f32 / 100.0;
    set_weapon_condition(g, actor, |c| c + d)?;
    none()
}

/// `actor.SetWeaponHealthPerc percent`: the equipped weapon's condition.
pub fn set_weapon_health_perc(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    // Never above 99.83 % [documented: GECK wiki SetWeaponHealthPerc, "hardcoded not to
    // increase condition beyond 99.83%"].
    let v = (a.num(0)? as f32 / 100.0).min(0.9983);
    set_weapon_condition(g, actor, |_| v)?;
    none()
}

/// 1 when the actor's equipped weapon is in the list [documented: catalogue summary].
pub fn is_weapon_in_list(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let list = a.form(0)?;
    let Some(w) = equipped_weapon(g, actor) else { return yes(false) };
    yes(super::refs::list_members(g, list).contains(&w))
}

/// 1 when the weapon (the subject itself when it is a weapon, else the subject's
/// equipped weapon [hypothesis for the second case]) uses the given skill (WEAP DNAM
/// skill) [documented: catalogue summary].
pub fn is_weapon_skill_type(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let skill = a.code(0)? as i64;
    let base = super::base_of(g, r);
    let w = match base.filter(|b| g.data.sig(*b).as_ref() == Some(b"WEAP")) {
        Some(b) => Some(b),
        None => equipped_weapon(g, r),
    };
    yes(w.and_then(|w| g.data.weapon_skills.get(&w)).copied() == Some(skill))
}

/// `ref.ResetInventory`: a fresh roll of the base's items replaces the inventory
/// [documented: catalogue summary].
pub fn reset_inventory(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.world.reset_inventory(r).map_err(|e| e.to_string())?;
    g.clear_npc_weapon_mods_roll(r);
    g.maybe_apply_npc_weapon_mods(r);
    none()
}

/// The number of items the container holds [documented: catalogue summary].
pub fn get_container_inventory_count(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let s = g.world.get(r).map_err(|e| e.to_string())?;
    Ok(Value::Num(s.inventory.as_ref().map(|i| i.iter().map(|(_, n)| n as f64).sum()).unwrap_or(0.0)))
}

/// `item.SetItemValue value`: the base item's value for every reference [documented:
/// catalogue summary]; kept with the form changes.
pub fn set_item_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let base = super::base_of(g, r).unwrap_or(r);
    g.forms.item_values.insert(base, a.int(0)?);
    none()
}

/// `SetQuestObject form flag`: the form's quest-item flag [documented: catalogue
/// summary]. It replaces the record's "Quest Item" header flag in the simulation
/// (`fnv_sim::World::set_quest_object`, the one source of truth, saved with the world):
/// quest items stay with the player in RemoveAllTypedItems and the player cannot drop
/// them by hand (a scripted `Drop` still can, as in the game). `FormChanges::quest_items`
/// is no longer written.
pub fn set_quest_object(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let f = a.form(0)?;
    g.world.set_quest_object(f, a.num_or(1, 1.0) != 0.0);
    none()
}

/// `AddItemToLeveledList list form level count health`: from now on `form` (an item or
/// a leveled list) is an entry of the leveled item list `list` at `level`, `count` at a
/// time; kept in the saved state and never taken back [documented: GECK wiki
/// AddItemToLeveledList] (`fnv_sim::World::add_to_leveled_list`). The health is a
/// percentage, 0–100 in New Vegas [documented: same page, "now accepts values from 0
/// to 100, rather than from 0 to 1"]: the items the entry gives have that condition
/// (`LeveledEntry::condition`); 100, or none given, is a whole item. The shipped calls
/// ask for 1 or 100 [observed: the 20 decoded calls of Honest Hearts and Old World
/// Blues]: 1 gives items at 1 % as the page reads [observed: capture `leveled_list_health`,
/// 2026-10-18: Vance's 9mm SMG drawn from a list entry added at 50 had 375 of 750 health,
/// at 1 had 7.5].
/// Level `0` is stored verbatim and integral levels convert into the 16-bit `LVLO` level
/// field by low-16-bit wrap (`(raw_level as i64) as u16`, e.g. `0 -> 0`, `1 -> 1`,
/// `65535 -> 0xFFFF`, `70000 -> 4464`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE
/// 6.4.9, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
/// isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game
/// install or Saves), `w4d_41_vanilla_player_and_lists`
/// (`tools/capture/nvse/catalogs/wave4d_extenders.json`, PR #3803 / #3802):
/// `PROBE|w4d_41_vanilla_player_and_lists|xp(0->25)|modAttr(5->7)|lvli(n0=4,l0=5:0,l1=6:1,l65k=7:-1,l70k=8:4464)|pkg(doc=14,pl=0)|rmMerch=ok`;
/// zero DLL, PDB, or third-party source consulted — `GetNumLevItems` on `0x00000AA1` grew
/// `4->5->6->7->8` and `GetNthLevItemLevel` read back `0`, `1`, `-1` (`0xFFFF`), and
/// `4464`]. Non-finite levels yield `0` [design]; count below `1` stays clamped to `1`
/// [hypothesis].
pub fn add_item_to_leveled_list(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let list = a.form(0)?;
    let item = a.form(1)?;
    let raw_level = a.num_or(2, 1.0);
    let level = if raw_level.is_finite() { (raw_level as i64) as u16 } else { 0 };
    let count = a.num_or(3, 1.0).clamp(1.0, u16::MAX as f64) as u16;
    let condition = a.num(4).ok().map(|h| (h / 100.0).clamp(0.0, 1.0) as f32).filter(|c| *c < 1.0);
    g.world.add_to_leveled_list(list, fnv_sim::forms::LeveledEntry { level, item, count, condition }).map_err(|e| e.to_string())?;
    none()
}

/// Run an item call, recording what it changed of the player's items as the host's story
/// when it counts (`crate::coop_story`).
fn story_items(g: &mut Game, ctx: &Ctx, f: impl FnOnce(&mut Game) -> Result<Value, String>) -> Result<Value, String> {
    // Any call may change the player's items (`container.RemoveAllItems player` gives
    // to it), so every one is watched while the story counts.
    if !g.story_counts(ctx) {
        return f(g);
    }
    let before = crate::coop_story::player_groups(g);
    let r = f(g);
    let after = crate::coop_story::player_groups(g);
    if before != after {
        g.coop_story.record_items(&before, &after);
        tracing::info!("co-op story: {} changed the player's items: {} kinds now recorded", ctx.unit, g.coop_story.items.len());
    }
    r
}

#[cfg(test)]
mod add_item_to_leveled_list_tests {
    use super::*;
    use crate::common::*;
    use crate::game::{GameConfig, RunCtx};
    use crate::GameData;
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_script::vm::XVal;
    use fnv_script::FunctionTable;
    use fnv_sim::{FormStore, RefTable, SimConfig, World};

    const ITEM_A: u32 = 0x801;
    const ITEM_B: u32 = 0x802;
    const LIST_ID: u32 = 0x810;

    struct Fixture {
        data: GameData,
        forms: FormStore,
        refs: RefTable,
    }

    fn fixture() -> Fixture {
        let misc = [
            record(b"MISC", 0, ITEM_A, &[sub(b"EDID", z("OwnedCap"))]),
            record(b"MISC", 0, ITEM_B, &[sub(b"EDID", z("OwnedChip"))]),
        ]
        .concat();
        let lvlo = [1u16.to_le_bytes().to_vec(), vec![0; 2], ITEM_A.to_le_bytes().to_vec(), 1u16.to_le_bytes().to_vec(), vec![0; 2]].concat();
        let lvli = record(b"LVLI", 0, LIST_ID, &[sub(b"EDID", z("OwnedLootList")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]), sub(b"LVLO", lvlo)]);
        let bytes = [tes4(), top(b"MISC", &misc), top(b"LVLI", &lvli)].concat();
        let plugins = [Plugin::parse(&bytes, "OwnedLvli.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = Schema::load_dir(root.join("fnv")).unwrap();
        let path = root.join("script/functions.json");
        let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let forms = FormStore::build(&lo, &schema);
        let refs = RefTable::build(&lo, &schema);
        let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
        Fixture { data, forms, refs }
    }

    fn game(f: &Fixture) -> Game<'_> {
        Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() })
    }

    #[test]
    fn add_item_to_leveled_list_accepts_zero_and_wraps_u16_levels() {
        use fnv_script::vm::ArgValue;

        let f = fixture();
        let mut g = game(&f);
        let ctx = Ctx { unit: "test".into(), opcode: 0x1236, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        assert_eq!(g.world.leveled_list(FormId(LIST_ID)).unwrap().entries.len(), 1);

        // Matches w4d_41_vanilla_player_and_lists: 0, 1, 65535, and 70000 all append entries.
        for (level, count, health) in [(0.0, 1.0, 100.0), (1.0, 2.0, 50.0), (65535.0, 1.0, 100.0), (70000.0, 0.0, 100.0)] {
            let vals = [
                ArgValue::Value(Value::Ref(LIST_ID)),
                ArgValue::Value(Value::Ref(ITEM_B)),
                ArgValue::Value(Value::Num(level)),
                ArgValue::Value(Value::Num(count)),
                ArgValue::Value(Value::Num(health)),
            ];
            add_item_to_leveled_list(&mut g, &ctx, &Args(&vals)).unwrap();
        }
        // Non-finite level does not panic and maps to 0.
        let nan_vals = [
            ArgValue::Value(Value::Ref(LIST_ID)),
            ArgValue::Value(Value::Ref(ITEM_B)),
            ArgValue::Value(Value::Num(f64::NAN)),
            ArgValue::Value(Value::Num(1.0)),
        ];
        add_item_to_leveled_list(&mut g, &ctx, &Args(&nan_vals)).unwrap();

        let entries = g.world.leveled_list(FormId(LIST_ID)).unwrap().entries;
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[1].level, 0, "level 0 stored verbatim");
        assert_eq!(entries[2].level, 1);
        assert_eq!(entries[2].count, 2);
        assert_eq!(entries[2].condition, Some(0.5));
        assert_eq!(entries[3].level, 65535);
        assert_eq!(entries[4].level, 70000u32 as u16, "70000 wraps into u16 (4464)");
        assert_eq!(entries[4].count, 1, "count below 1 stays clamped to 1");
        assert_eq!(entries[5].level, 0, "non-finite level maps to 0");

        // Verify GetNumLevItems / GetNthLevItemLevel readback through the script runtime.
        let num_items = g.run_query("Begin Function { }\nSetFunctionValue (GetNumLevItems OwnedLootList)\nEnd\n", RunCtx::default()).unwrap();
        assert_eq!(num_items, XVal::Num(6.0));
        let lv_zero = g.run_query("Begin Function { }\nSetFunctionValue (GetNthLevItemLevel OwnedLootList 1)\nEnd\n", RunCtx::default()).unwrap();
        assert_eq!(lv_zero, XVal::Num(0.0));
        let lv_wrap = g.run_query("Begin Function { }\nSetFunctionValue (GetNthLevItemLevel OwnedLootList 4)\nEnd\n", RunCtx::default()).unwrap();
        assert_eq!(lv_wrap, XVal::Num(4464.0));
    }
}
