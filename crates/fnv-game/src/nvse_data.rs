//! xNVSE's mod local data, GetUserTime, inventory references and temporary clones
//! [documented: their pages; observed: the peer's nvse-data and nvse-inv probes in the
//! original game, 2026-10-26].

use crate::game::Game;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;

/// Clone ids: from here for a clone that belongs to no plugin (GetSourceModIndex 255);
/// the calling script's (mod index 0 for a runtime script) from [`CLONE_LOCAL`]
/// [observed: TempCloneForm with and without indexToCallingScript].
pub const CLONE_BASE: u32 = 0xFE00_0000;
pub const CLONE_LOCAL: u32 = 0x00FE_0000;

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

impl Game<'_> {
    /// The original a clone was made from, through clones of clones; the form itself
    /// otherwise.
    pub fn unclone(&self, mut f: FormId) -> FormId {
        while let Some(&o) = self.clones.get(&f) {
            f = o;
        }
        f
    }

    /// Read-only view of active `TempCloneForm` alias -> parent mappings.
    pub fn clones(&self) -> &std::collections::HashMap<FormId, FormId> {
        &self.clones
    }

    /// Snapshot `source`'s session `MODL/` override (if any) onto `clone` at
    /// `TempCloneForm` creation time so pre-clone `SetModelPath` / `SetIdleAnimPath`
    /// edits carry onto `clone`, while post-clone edits to `source` remain isolated
    /// [observed: `w4g_02_tempcloneform_world_proj_csty`,
    /// `w4g_13_isidleplaying_and_idle_replace`].
    pub(crate) fn snapshot_model_path(&mut self, source: FormId, clone: FormId) {
        if let Some(val) = self.field_overrides.get(&(source, "MODL/".into())).cloned() {
            self.field_overrides.insert((clone, "MODL/".into()), val);
        }
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let v = match name {
        // Keys without regard to case; numbers, strings and forms [observed: nvse-data;
        // the values last for the session, hypothesis].
        "setmodlocaldata" => {
            let v = args.get(1).cloned().unwrap_or(XVal::Num(0.0));
            g.mod_data.insert(text(args, 0).to_lowercase(), (text(args, 0), v));
            XVal::Num(1.0)
        }
        "getmodlocaldata" => g.mod_data.get(&text(args, 0).to_lowercase()).map_or(XVal::Num(0.0), |(_, v)| v.clone()),
        "modlocaldataexists" => yes(g.mod_data.contains_key(&text(args, 0).to_lowercase())),
        "removemodlocaldata" => yes(g.mod_data.remove(&text(args, 0).to_lowercase()).is_some()),
        "getallmodlocaldata" => {
            let entries = g.mod_data.values().map(|(k, v)| (XKey::Str(k.clone()), v.clone())).collect();
            XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
        }
        // [documented: GetUserTime; observed: a string map of the 8 keys] the computer's
        // local time.
        "getusertime" => {
            let now = jiff::Timestamp::now().to_zoned_datetime(crate::local_time::system_zone());
            let pairs: [(&str, u32); 8] = [
                ("Year", now.year().max(0) as u32),
                ("Month", now.month() as u32),
                ("DayOfWeek", now.weekday().to_sunday_zero_offset() as u32),
                ("Day", now.day() as u32),
                ("Hour", now.hour() as u32),
                ("Minute", now.minute() as u32),
                ("Second", now.second() as u32),
                ("Millisecond", (now.millisecond() as u32).min(999)),
            ];
            let entries = pairs.iter().map(|(k, v)| (XKey::Str((*k).to_string()), XVal::Num(f64::from(*v)))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
        }
        // [documented: IsInventoryRef; observed: 1 on foreach's reference, 0 on a world
        // reference].
        "isinventoryref" => yes(subject.is_some_and(|r| g.temp_item(r).is_some())),
        // [documented: GetInvRefsForItem] the container's inventory references of an item:
        // one per kind of item here [hypothesis: the original may give one per stack].
        "getinvrefsforitem" => {
            let item = form(args, 0).unwrap_or(FormId(0));
            let c = subject.unwrap_or(FormId(0));
            let has = g.world.inventory_mut(c).map(|inv| inv.iter().any(|(i, n)| i == item && n > 0)).unwrap_or(false);
            let refs = if has { vec![XVal::Ref(g.temp_ref(c, item).0)] } else { Vec::new() };
            let entries = refs.into_iter().enumerate().map(|(i, r)| (XKey::Num(i as f64), r)).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // [documented: IsEquipped; observed: 1 after EquipItem, 0 after UnequipItem] on an
        // inventory reference: its item is worn by its container.
        "isequipped" => match subject.and_then(|r| g.temp_item(r)) {
            Some((c, item)) => yes(g.world.inventory_mut(c).is_ok_and(|inv| inv.is_equipped(item))),
            None => XVal::Num(0.0),
        },
        // [documented: TempCloneForm; IsClonedForm; observed: the clone keeps the
        // original's name, type and fields] a new form standing for the original, for the
        // session.
        "tempcloneform" => {
            // [documented: TempCloneForm] An omitted form uses the caller's base;
            // explicit null retains its existing no-clone result.
            let original = if args.is_empty() { subject.and_then(|r| crate::functions::base_of(g, r)) } else { form(args, 0) };
            let Some(f) = original else { return Some(Ok(XVal::Ref(0))) };
            let local = args.get(1).is_some_and(XVal::truthy);
            let id = match g.next_clone_identity(local) {
                Ok(id) => id,
                Err(error) => return Some(Err(error)),
            };
            g.snapshot_combat_style(f, id);
            g.snapshot_sound_traits(f, id);
            g.snapshot_explosion_traits(f, id);
            g.snapshot_sound_flags(f, id);
            g.snapshot_magic_traits(f, id);
            g.snapshot_magic_effects(f, id);
            g.snapshot_ammo_traits(f, id);
            g.snapshot_projectile_traits(f, id);
            g.snapshot_weapon_sounds(f, id);
            g.snapshot_object_effect(f, id);
            g.snapshot_weapon_crit_damage(f, id);
            g.snapshot_model_path(f, id);
            g.clones.insert(id, f);
            // A clone of an item goes into containers (MCM's list of mods, `World`).
            g.world.add_clone_item(id, f);
            g.snapshot_armor_overrides(f, id);
            XVal::Ref(id.0)
        }
        "isclonedform" => yes(form(args, 0).or(subject).is_some_and(|f| g.clones.contains_key(&f))),
        _ => return None,
    };
    Some(Ok(v))
}
