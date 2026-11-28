//! Furniture state functions (docs/spec-notes/furniture.md §2–3): what an actor on a
//! chair, stool or bed answers. The state itself is set by whoever moves the actor
//! (the engine, AI packages) through [`Game::set_furniture_use`].

use super::{base_of, none, yes, Args, Ctx};
use crate::game::{FurnitureUse, Game};
use fnv_script::vm::Value;
use fnv_sim::FormId;

impl Game<'_> {
    /// The furniture `actor` is using now, or `None` when it stops.
    pub fn set_furniture_use(&mut self, actor: FormId, using: Option<FurnitureUse>) {
        if using.is_none() && !self.extra.contains_key(&actor) {
            return;
        }
        self.extra.entry(actor).or_default().furniture = using;
    }

    /// Whether an actor other than `actor` is on (or getting on or off) the furniture
    /// `reference`: the player or any actor the engine seats, not only the AI's own
    /// (`fnv_ai`'s kept furniture). A seat the player took is not free for a Find or Use
    /// Item At package (the G.O.A.T., CG03: Paul went to the desk the player sat at).
    pub fn furniture_used_by_other(&self, actor: FormId, reference: FormId) -> bool {
        self.extra.iter().any(|(a, e)| *a != actor && e.furniture.is_some_and(|u| u.reference == reference))
    }

    /// A load stands every actor up, the player and each NPC alike: the saved furniture
    /// use is cleared, so GetSitting and its kin read 0 (and the furniture is free) until
    /// the engine seats the body again and says so through [`Game::set_furniture_use`]
    /// (`furniture: … goes to` / `is back on`). The engine's seats and the AI's furniture
    /// in use are not saved (docs/design/save-load.md §3.3, §3.4), and a load starts a new
    /// run, so no body is on furniture as the game's state is restored [design, #2797;
    /// the player's half #1573]. Called by [`Game::restore`].
    ///
    /// Each NPC the save had on its seat (state 3, not getting on or off) is noted in
    /// [`Game::saved_seats`], for the engine to put back on that furniture at once, with
    /// no walk and no getting-on clip, when its body is drawn and its package still wants
    /// it there [observed: observer check 220, base game 2026-11-25: Easy Pete saved on
    /// his porch chair read GetSitting 3 at the saved spot from the first sample after the
    /// load, about 2 s, through 20 s; #3991]. The player stands up as before (#1573).
    pub(crate) fn stand_everyone_up(&mut self) {
        self.saved_seats.clear();
        for (actor, e) in self.extra.iter_mut() {
            if let Some(u) = e.furniture.take() {
                if *actor != FormId::PLAYER_REF && u.state == SEATED {
                    self.saved_seats.insert(*actor, u.reference);
                }
            }
        }
    }

    /// The seats a load found NPCs on ([`Game::stand_everyone_up`]), once.
    pub fn take_saved_seats(&mut self) -> std::collections::BTreeMap<FormId, FormId> {
        std::mem::take(&mut self.saved_seats)
    }

    pub fn furniture_use(&self, actor: FormId) -> Option<FurnitureUse> {
        self.extra.get(&actor).and_then(|e| e.furniture)
    }
}

fn using(g: &Game, ctx: &Ctx) -> Result<Option<FurnitureUse>, String> {
    Ok(g.furniture_use(ctx.subject()?))
}

/// Marker IDs 1 to 10 are beds [documented: GECK wiki GetFurnitureMarkerID, catalogue
/// notes]: their users sleep, the others sit.
/// Marker 10 is the crucifix's, sat on (`fnv_actor::furniture::is_bed`): 1 to 9 here.
pub(crate) fn bed(u: &FurnitureUse) -> bool {
    (1..=9).contains(&u.marker_id)
}

/// GetSitting's and GetSleeping's state while on the furniture [observed: furniture.md §3].
pub(crate) const SEATED: u8 = 3;

/// 0 when not on a seat; else 2 while getting on, 3 while on it, 4 while getting off
/// [observed: the IDLE tree and the AI observer, furniture.md §3].
pub fn get_sitting(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(Value::Num(using(g, ctx)?.filter(|u| !bed(u)).map_or(0.0, |u| u.state as f64)))
}

/// As [`get_sitting`], for beds.
pub fn get_sleeping(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(Value::Num(using(g, ctx)?.filter(bed).map_or(0.0, |u| u.state as f64)))
}

pub fn get_furniture_marker_id(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(Value::Num(using(g, ctx)?.map_or(0.0, |u| u.marker_id as f64)))
}

pub fn is_current_furniture_ref(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let Some(r) = a.opt_form(0) else { return none() };
    let list = super::refs::list_members(g, r);
    yes(using(g, ctx)?.is_some_and(|u| u.reference == r || list.contains(&u.reference)))
}

pub fn is_current_furniture_obj(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let Some(b) = a.opt_form(0) else { return none() };
    // Furniture given directly or as a form list: NVDance asks it of DanceMarkerObjects
    // (furniture.md §3).
    let list = super::refs::list_members(g, b);
    yes(using(g, ctx)?.and_then(|u| base_of(g, u.reference)).is_some_and(|base| base == b || list.contains(&base)))
}

/// What `actor` uses now: the base of the furniture it is on, else the item its package
/// last had it use or take (`RefExtra::used_item`, set by `fnv_ai`'s Find and Use Item At
/// packages) [hypothesis: the documentation says only "using" an item, GECK wiki
/// GetIsUsedItem; the shipped conditions ask it of seated actors (furniture.md §8)].
fn used(g: &Game, actor: FormId) -> Option<FormId> {
    let e = g.extra.get(&actor)?;
    e.furniture.and_then(|u| base_of(g, u.reference)).or(e.used_item)
}

/// Form type codes of what can be used (GetIsUsedItemType): the inventory types
/// `items::form_type_code` gives, and ACTI 21, TACT 22, TERM 23, CONT 27, DOOR 28,
/// FLOR 38, FURN 39 [hypothesis: the same numbering continued; a shipped idle condition
/// asks `GetIsUsedItemType 0x15` of seated actors, furniture.md §8].
pub(crate) fn used_type_code(sig: &[u8; 4]) -> Option<i32> {
    Some(match sig {
        b"ACTI" => 21,
        b"TACT" => 22,
        b"TERM" => 23,
        b"CONT" => 27,
        b"DOOR" => 28,
        b"FLOR" => 38,
        b"FURN" => 39,
        _ => return super::items::form_type_code(sig),
    })
}

/// The type names GetIsUsedItemType takes in a script ("The full name of the Form Type
/// must be passed" [documented: GECK wiki GetIsUsedItemType]), for the types above.
const TYPE_NAMES: [(&str, &[u8; 4]); 17] = [
    ("Activator", b"ACTI"),
    ("TalkingActivator", b"TACT"),
    ("Terminal", b"TERM"),
    ("Armor", b"ARMO"),
    ("Book", b"BOOK"),
    ("Container", b"CONT"),
    ("Door", b"DOOR"),
    ("Ingredient", b"INGR"),
    ("Light", b"LIGH"),
    ("Misc", b"MISC"),
    ("Flora", b"FLOR"),
    ("Furniture", b"FURN"),
    ("Weapon", b"WEAP"),
    ("Ammo", b"AMMO"),
    ("Key", b"KEYM"),
    ("AlchemyItem", b"ALCH"),
    ("Note", b"NOTE"),
];

/// `actor.GetIsUsedItem item`: 1 when what the actor uses ([`used`]) is `item`
/// [documented: GECK wiki GetIsUsedItem, "Checks if an actor is using a specified item"].
pub fn get_is_used_item(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let Some(item) = a.opt_form(0) else { return none() };
    yes(used(g, ctx.subject()?) == Some(item))
}

/// `GetIsUsedItemType type`: 1 when what the subject uses is of that form type (a code,
/// or a type name) [documented: GECK wiki GetIsUsedItemType].
pub fn get_is_used_item_type(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let Some(sig) = used(g, ctx.subject()?).and_then(|b| g.data.sig(g.unclone(b))) else { return none() };
    let want = match a.peek_str(0) {
        Some(name) => TYPE_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).and_then(|(_, s)| used_type_code(s)),
        None => a.num(0).ok().map(|c| c as i32),
    };
    yes(want.is_some() && used_type_code(&sig) == want)
}

/// `GetIsUsedItemEquipType type`: 1 when what the subject uses is an ingestible of that
/// equip type (ETYP: 10 Chems, 11 Stimpak, 12 Food, 13 Alcohol) [documented: GECK wiki
/// GetIsUsedItemEquipType; hypothesis: only ingestibles carry an equip type here].
pub fn get_is_used_item_equip_type(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let Some(item) = used(g, ctx.subject()?) else { return none() };
    let want = a.num(0)? as i64;
    yes(g.ingestible_def(item).is_some_and(|d| d.equip_type == want))
}

/// `actor.GetUsedItemActivate`: 1 while the actor uses something ([`used`])
/// [documented: GECK wiki GetUsedItemActivate, "Returns true if the actor is
/// using/activating an item(?)"; the page is marked incomplete]. Inside the walk of the
/// player's activation idle ([`Game::activate_idle`]) it is also 1 while that actor is
/// activating something ([`Game::activating`], the window IsWaterObject reads), so the
/// tree's `ActivateIdles` root, whose one condition it is, opens for the drink (the
/// wiki's notes: "to check whether an actor is activating something"; idles.md §16)
/// [documented: fnvtool esm decode, FalloutNV.esm 0x00003BDC; design, #2349: everywhere
/// else, scripts, quests, dialogue, packages and the NPCs' idle walks, its answer is
/// unchanged].
pub fn get_used_item_activate(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let s = ctx.subject()?;
    yes(used(g, s).is_some() || (g.activate_walk == Some(s) && g.activating(s).is_some()))
}
