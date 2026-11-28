//! The Anh NVSE plugin's functions on the game: armor rating, quest completion and
//! encounter zones [documented: each page in the wiki export; docs/spec-notes/ext-anh.md].
//! Zone level/min-level/owner round-trips are `[observed]` on 2026-11-23 in
//! `GSDocMitchellHouse` (`0x00103DF9`) with Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9,
//! AnhNVSE 131, and the 10 official base masters. Changes last for the session.

use crate::game::{Game, RunCtx};
use fnv_script::vm::XVal;
use fnv_sim::FormId;

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => 0.0,
    }
}

impl Game<'_> {
    /// An encounter zone's record (ECZN DATA) with saved `SetZoneRespawns`
    /// (`World::zone_respawns_override`) and session `SetZoneMinLevel`,
    /// `SetZoneOwner`, and `SetZoneFlag` (`nvse_lutana`) changes.
    pub(crate) fn zone(&self, z: FormId) -> Option<fnv_sim::forms::ZoneDef> {
        let mut d = self.world.effective_zone_def(z)?;
        if let Some(respawns) = self.world.zone_respawns_override(z) {
            d.never_resets = !respawns;
        }
        if let Some(c) = self.other.zone_changes.get(&z) {
            if let Some(o) = c.owner {
                d.owner = (o.0 != 0).then_some(o);
            }
            d.never_resets = c.never_resets.unwrap_or(d.never_resets);
        }
        Some(d)
    }
}

/// SetZoneOwner's and session SetZoneFlag bit-1 (`nvse_lutana`) changes to a zone
/// (`min_level` and `match_pc_below_min` live on `fnv_sim::World` / `WorldSession`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ZoneChange {
    pub owner: Option<FormId>,
    pub never_resets: Option<bool>,
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // `[armor]` and `newDR [armor]`: as GetArmorArmorRating and SetArmorArmorRating,
        // the argument else the calling reference's base [documented:
        // GetArmorArmorRatingAlt, alias GetArmorARAlt; SetArmorArmorRatingAlt, alias
        // SetArmorARAlt].
        "getarmorarmorratingalt" | "getarmoraralt" => return crate::nvse::call(g, ctx, "getarmorarmorrating", subject, args),
        "setarmorarmorratingalt" | "setarmoraralt" => return crate::nvse::call(g, ctx, "setarmorarmorrating", subject, args),
        // 1 when the quest is completed and not failed [documented: GetQuestCompletedAlt].
        "getquestcompletedalt" => {
            let s = form(args, 0).and_then(|q| g.quests.get(&q));
            XVal::Num(f64::from(u8::from(s.is_some_and(|s| s.completed && !s.failed))))
        }
        // [documented: GetZoneLevel, "the current level"; observed: 2026-11-23,
        // GSDocMitchellHouse (0x00103DF9), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
        // AnhNVSE 131 with the 10 official base masters, probe
        // `anh_07_zone_level_min_owner`: 0 on unentered Vault11Zone (000E9081)].
        "getzonelevel" => XVal::Num(form(args, 0).and_then(|z| g.zone_level(z)).map_or(0.0, f64::from)),
        // [documented: GetZoneMinLevel; GetZoneMatchLevel, "Match PC Below Minimum Level";
        // GetZoneOwner].
        "getzoneminlevel" => XVal::Num(form(args, 0).and_then(|z| g.zone(z)).map_or(0.0, |d| f64::from(d.min_level))),
        "getzonematchlevel" => XVal::Num(form(args, 0).and_then(|z| g.zone(z)).map_or(0.0, |d| f64::from(u8::from(d.match_pc_below_min)))),
        "getzoneowner" => XVal::Ref(form(args, 0).and_then(|z| g.zone(z)).and_then(|d| d.owner).map_or(0, |o| o.0)),
        // `zone level` and `zone owner` [documented: SetZoneMinLevel; SetZoneOwner;
        // observed: 2026-11-23 & 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
        // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + AnhNVSE 131 with the 10 official
        // base masters, probes `anh_07_zone_level_min_owner` (0,6,12),
        // `anh_11_zone_owner_readback` (Vault11Zone 000E9081: 00000000 -> 000A46E7 ->
        // 00104C6E -> 00104C6E; null owner argument leaves the existing owner unchanged),
        // and `w4d_29_anh_scancode_table_and_zone` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
        // #3802: `zone(0->0,docZ=00000000)`:
        // `GetZoneLevel` on unentered `0xE9081` stays 0 after `SetZoneMinLevel 15`); no
        // DLL or source consulted]; session-only lifetime and keeping an already-entered
        // zone's level remain [hypothesis].
        "setzoneminlevel" | "setzoneowner" => {
            if let Some(z) = form(args, 0).filter(|z| g.world.forms.zones.contains_key(z)) {
                if name == "setzoneminlevel" {
                    g.world.set_zone_min_level(z, num(args, 1).clamp(f64::from(i8::MIN), f64::from(i8::MAX)) as i8);
                } else if let Some(owner) = form(args, 1) {
                    g.other.zone_changes.entry(z).or_default().owner = Some(owner);
                }
            }
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}
