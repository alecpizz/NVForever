//! Optional random weapon mods on NPC main weapons (`npc-weapon-mods`, #3876).
//!
//! Off by default: when disabled, NPC weapons are untouched (zero RNG, zero mod flags,
//! zero `RefExtra` writes). When enabled, each non-player NPC (`NPC_`, including placed
//! or spawned leveled `LVLN` picks) rolls once when its main weapon is outfitted to
//! attach 1..=3 weapon mods chosen from the weapon's valid `WMI1`-`WMI3` mod slots.
//!
//! - Chance formula and default parameters (`uWMChancePerLevel=2`, `uWMChanceMin=10`,
//!   `uWMChanceMax=60`, `Chance(%) = Min(60, Max(10, Level * 2))`) follow JIP LN NVSE
//!   Plugin `57.30` (`https://www.nexusmods.com/newvegas/mods/58277`) `jip_nvse.ini`
//!   `[GamePatches]` `bNPCWeaponMods` [documented: JIP LN NVSE Plugin 57.30
//!   `jip_nvse.ini`; no DLL or source consulted; `docs/spec-notes/npc-weapon-mods.md`].
//! - NVForever never reads `jip_nvse.ini` (`docs/design/extender-compat.md` §8.4).
//! - Deterministic per-NPC SplitMix64 seed, conditional multi-mod probability curve
//!   (75%/20%/5% for 3-slot weapons, 80%/20% for 2-slot weapons), and single-roll-per-NPC
//!   tracking (`RefExtra::npc_weapon_mods_rolled`) are NVForever's own [design].

use crate::game::Game;
use fnv_script::vm::XVal;
use fnv_sim::FormId;

/// Chance percent increase per NPC level [documented: JIP LN NVSE Plugin 57.30
/// `jip_nvse.ini` `[GamePatches]` `uWMChancePerLevel=2`; no DLL or source consulted].
pub const CHANCE_PER_LEVEL: u32 = 2;

/// Minimum chance percent for an NPC's main weapon to include weapon mods [documented:
/// JIP LN NVSE Plugin 57.30 `jip_nvse.ini` `[GamePatches]` `uWMChanceMin=10`; no DLL or
/// source consulted].
pub const CHANCE_MIN: u32 = 10;

/// Maximum chance percent for an NPC's main weapon to include weapon mods [documented:
/// JIP LN NVSE Plugin 57.30 `jip_nvse.ini` `[GamePatches]` `uWMChanceMax=60`; no DLL or
/// source consulted].
pub const CHANCE_MAX: u32 = 60;

/// Level-scaled chance percent for an NPC's main weapon to include at least one weapon
/// mod: `Min(60, Max(10, level * 2))` [documented: JIP LN NVSE Plugin 57.30
/// `jip_nvse.ini` `[GamePatches]` formula `MinOf(uWMChanceMax, MaxOf(uWMChanceMin,
/// Level * uWMChancePerLevel))`; no DLL or source consulted].
pub fn chance_pct(level: u32) -> u32 {
    level.saturating_mul(CHANCE_PER_LEVEL).clamp(CHANCE_MIN, CHANCE_MAX)
}

/// SplitMix64 mixing step for deterministic per-NPC weapon mod rolls [design].
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministically roll a weapon mod bitmask (subset of `available_mask & 0b111`) for
/// one NPC's main weapon [design: deterministic SplitMix64 seed and conditional multi-mod
/// curve following JIP LN 57.30 `jip_nvse.ini`'s "though 2 mods is rare, and a
/// fully-modded weapon is uncommon"].
///
/// - Returns `0` immediately when `available_mask & 0b111 == 0`.
/// - Overall probability of returning a non-zero mask is `chance_pct(level)%`.
/// - Given a modded roll:
///   - 1 valid slot: 1 mod (100%).
///   - 2 valid slots: 1 mod (80%), 2 mods (20%).
///   - 3 valid slots: 1 mod (75%), 2 mods (20%), 3 mods (5%).
pub fn roll_weapon_mod_mask(seed: u64, actor: FormId, weapon: FormId, rolls: u32, level: u32, available_mask: u8) -> u8 {
    let mut slots = [0u8; 3];
    let mut n = 0usize;
    for bit in [1u8, 2u8, 4u8] {
        if available_mask & bit != 0 {
            slots[n] = bit;
            n += 1;
        }
    }
    if n == 0 {
        return 0;
    }

    // Mix world seed, actor FormID, weapon FormID, and resurrection/reset roll count into
    // an independent per-NPC stream so vanilla leveled-list RNG is never perturbed [design].
    let mut state = seed
        ^ (u64::from(actor.0).wrapping_mul(0xD6E8_FEB8_6659_FD93))
        ^ (u64::from(weapon.0).rotate_left(17).wrapping_mul(0xA5A3_564E_27F8_862D))
        ^ (u64::from(rolls).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        ^ 0x4E50_4357_4D4F_4453; // "NPCWMODS" domain tag [design]

    let roll_chance = splitmix64(&mut state) % 100;
    if roll_chance >= u64::from(chance_pct(level)) {
        return 0;
    }

    let count = match n {
        1 => 1,
        2 => {
            let r = splitmix64(&mut state) % 100;
            if r < 80 { 1 } else { 2 }
        }
        _ => {
            let r = splitmix64(&mut state) % 100;
            if r < 75 {
                1
            } else if r < 95 {
                2
            } else {
                3
            }
        }
    };

    // Fisher-Yates shuffle over the available slots and take `count` bits.
    for i in (1..n).rev() {
        let j = (splitmix64(&mut state) as usize) % (i + 1);
        slots.swap(i, j);
    }
    let mut mask = 0u8;
    for &bit in &slots[..count] {
        mask |= bit;
    }
    mask
}

impl Game<'_> {
    /// Whether `r` resolves to a non-player `NPC_` actor (following placed/spawned `LVLN`
    /// leveled actor lists via [`Game::template_source`], excluding the player and `CREA`).
    fn is_non_player_npc(&self, r: FormId) -> bool {
        if r == FormId::PLAYER_REF {
            return false;
        }
        let Some(base) = crate::functions::base_of(self, r) else {
            return false;
        };
        self.template_source(base, r, |_| false).is_some_and(|(_, def)| def.sig == *b"NPC_")
    }

    /// Bitmask of valid weapon mod slots (`1` = `WMI1`, `2` = `WMI2`, `4` = `WMI3`) on
    /// `weapon`, honoring `TempCloneForm` clones and session `SetWeaponItemMod` overrides.
    pub fn available_weapon_mod_mask(&self, weapon: FormId) -> u8 {
        let mut orig = weapon;
        for _ in 0..16 {
            let Some(&p) = self.clones.get(&orig) else { break };
            orig = p;
        }
        let authored = self.world.forms.item(orig).and_then(|i| i.weapon_traits.as_ref()).map_or([None; 3], |t| t.mods);
        let mut mask = 0u8;
        for (idx, key) in ["WMI1/", "WMI2/", "WMI3/"].iter().enumerate() {
            let ov = self
                .field_overrides
                .get(&(weapon, (*key).into()))
                .or_else(|| self.field_overrides.get(&(orig, (*key).into())));
            let has_slot = match ov {
                Some(XVal::Ref(id)) => *id != 0,
                _ => authored[idx].is_some_and(|m| m.0 != 0),
            };
            if has_slot {
                mask |= 1u8 << idx;
            }
        }
        mask
    }

    /// Clear the per-NPC weapon mod roll marker without inserting into `self.extra` when
    /// absent (used when resetting an actor's inventory or resurrecting it fresh).
    pub(crate) fn clear_npc_weapon_mods_roll(&mut self, actor: FormId) {
        if let Some(e) = self.extra.get_mut(&actor) {
            e.npc_weapon_mods_rolled = false;
        }
    }

    /// Roll and attach random weapon mods on `actor`'s equipped main weapon when the
    /// off-by-default `npc-weapon-mods` option (`Tweaks::npc_weapon_mods`, #3876) is
    /// enabled. Returns the newly attached mod mask (`0` when disabled, ineligible,
    /// already rolled, already modded, non-moddable, or the roll missed).
    pub fn maybe_apply_npc_weapon_mods(&mut self, actor: FormId) -> u8 {
        if !self.tweaks.npc_weapon_mods {
            return 0;
        }
        if actor == FormId::PLAYER_REF {
            return 0;
        }
        if self.extra.get(&actor).is_some_and(|e| e.npc_weapon_mods_rolled) {
            return 0;
        }
        if !self.world.peek(actor).is_some_and(|s| s.outfitted) {
            return 0;
        }
        if !self.is_non_player_npc(actor) {
            return 0;
        }

        // Record single-roll-per-NPC so repeated ticks never re-roll this NPC [design].
        self.extra.entry(actor).or_default().npc_weapon_mods_rolled = true;

        let Some(weapon) = crate::functions::items::equipped_weapon(self, actor) else {
            return 0;
        };
        let Some(equipped_extra) = self
            .world
            .peek(actor)
            .and_then(|s| s.inventory.as_ref())
            .and_then(|inv| inv.groups().find(|(id, e, n)| *id == weapon && e.equipped && *n > 0).map(|(_, e, _)| e))
        else {
            return 0;
        };
        // Never overwrite an already-modded weapon (save, script, or leveled entry).
        if equipped_extra.mods != 0 {
            return 0;
        }

        let avail = self.available_weapon_mod_mask(weapon);
        if avail == 0 {
            return 0;
        }

        let level = self.actor_level(actor).round().max(1.0) as u32;
        let rolls = self.extra.get(&actor).map_or(0, |e| e.rolls);
        let seed = self.world.config().seed;
        let mask = roll_weapon_mod_mask(seed, actor, weapon, rolls, level, avail);
        if mask == 0 {
            return 0;
        }

        if let Ok(inv) = self.world.inventory_mut(actor) {
            if inv.set_mods_one(weapon, equipped_extra, mask) {
                return mask;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{group, record, sub, tes4, top, z};
    use crate::game::{Game, GameConfig};
    use crate::GameData;
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_script::FunctionTable;
    use fnv_sim::inventory::Extra;
    use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
    use std::collections::BTreeSet;

    fn imod(fid: u32, edid: &str) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend(50u32.to_le_bytes());
        data.extend(0.0f32.to_le_bytes());
        record(b"IMOD", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", data)])
    }

    fn weap(fid: u32, edid: &str, damage: u16, slots: [Option<u32>; 3]) -> Vec<u8> {
        // WEAP DATA: value(u32), health(u32), weight(f32), damage(u16), clip(u8) = 15 bytes
        let mut data = Vec::new();
        data.extend(100u32.to_le_bytes());
        data.extend(100u32.to_le_bytes());
        data.extend(4.0f32.to_le_bytes());
        data.extend(damage.to_le_bytes());
        data.push(10u8);
        // Minimal WEAP DNAM: type=3 (OneHandPistol, drawable), flags1=0, flags2=0
        let mut dnam = vec![0u8; 204];
        dnam[0..4].copy_from_slice(&3u32.to_le_bytes());
        let mut subs = vec![
            sub(b"EDID", z(edid)),
            sub(b"MODL", z("weapons\\test.nif")),
            sub(b"DATA", data),
            sub(b"DNAM", dnam),
        ];
        for (sig, slot) in [(b"WMI1", slots[0]), (b"WMI2", slots[1]), (b"WMI3", slots[2])] {
            if let Some(m) = slot {
                subs.push(sub(sig, m.to_le_bytes().to_vec()));
            }
        }
        record(b"WEAP", 0, fid, &subs)
    }

    fn acbs(level: u16) -> Vec<u8> {
        // ACBS is 24 bytes: flags(u32), fatigue(u16), barter(u16), level(i16), ...
        let mut b = vec![0u8; 24];
        b[8..10].copy_from_slice(&(level as i16).to_le_bytes());
        b
    }

    fn cnto(item: u32, count: i32) -> fnv_esm::raw::Subrecord {
        let mut b = Vec::new();
        b.extend(item.to_le_bytes());
        b.extend(count.to_le_bytes());
        sub(b"CNTO", b)
    }

    fn npc(fid: u32, edid: &str, level: u16, items: &[(u32, i32)]) -> Vec<u8> {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"ACBS", acbs(level))];
        for &(item, count) in items {
            subs.push(cnto(item, count));
        }
        record(b"NPC_", 0, fid, &subs)
    }

    fn crea(fid: u32, edid: &str, level: u16, items: &[(u32, i32)]) -> Vec<u8> {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"ACBS", acbs(level))];
        for &(item, count) in items {
            subs.push(cnto(item, count));
        }
        record(b"CREA", 0, fid, &subs)
    }

    fn lvln(fid: u32, edid: &str, npc_fid: u32) -> Vec<u8> {
        // LVLO: level(u16), pad(2), form(u32), count(u16), pad(2)
        let mut lvlo = Vec::new();
        lvlo.extend(1u16.to_le_bytes());
        lvlo.extend([0u8; 2]);
        lvlo.extend(npc_fid.to_le_bytes());
        lvlo.extend(1u16.to_le_bytes());
        lvlo.extend([0u8; 2]);
        record(b"LVLN", 0, fid, &[sub(b"EDID", z(edid)), sub(b"LVLF", vec![0]), sub(b"LVLO", lvlo)])
    }

    fn achr(fid: u32, base: u32) -> Vec<u8> {
        let mut data = vec![0u8; 24];
        data[0..4].copy_from_slice(&0.0f32.to_le_bytes());
        record(b"ACHR", 0, fid, &[sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
    }

    fn cell_with(cell_fid: u32, refs: &[Vec<u8>]) -> Vec<u8> {
        let cell_rec = record(b"CELL", 0, cell_fid, &[sub(b"EDID", z("TestCell")), sub(b"DATA", vec![1])]);
        let mut body = Vec::new();
        for r in refs {
            body.extend(r);
        }
        let temp = group(cell_fid.to_le_bytes(), 9, &body);
        let children = group(cell_fid.to_le_bytes(), 6, &temp);
        [cell_rec, children].concat()
    }

    struct Fixture {
        forms: FormStore,
        refs: RefTable,
        data: GameData,
    }

    impl Fixture {
        fn build(bytes: &[u8]) -> Self {
            let plugins = [Plugin::parse(bytes, "NpcWeaponModsTest.esm".into()).unwrap()];
            let lo = LoadOrder::new(&plugins).unwrap();
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
            let schema = Schema::load_dir(root.join("fnv")).unwrap();
            let path = root.join("script/functions.json");
            let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let forms = FormStore::build(&lo, &schema);
            let refs = RefTable::build(&lo, &schema);
            let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
            Self { forms, refs, data }
        }

        fn game(&self, seed: u64) -> Game<'_> {
            let cfg = SimConfig { seed, ..SimConfig::default() };
            Game::new(&self.data, World::new(&self.forms, &self.refs, cfg), GameConfig::default())
        }
    }

    #[test]
    fn chance_formula_matches_jip_ln_5730_bounds() {
        assert_eq!(chance_pct(0), 10);
        assert_eq!(chance_pct(1), 10);
        assert_eq!(chance_pct(5), 10);
        assert_eq!(chance_pct(6), 12);
        assert_eq!(chance_pct(10), 20);
        assert_eq!(chance_pct(25), 50);
        assert_eq!(chance_pct(30), 60);
        assert_eq!(chance_pct(50), 60);
        assert_eq!(chance_pct(u32::MAX), 60);
    }

    #[test]
    fn roll_curve_matches_documented_chance_and_multi_mod_rarity() {
        let weapon = FormId(0x2001);
        let samples = 20_000u32;

        // Non-moddable weapon always yields 0.
        for i in 0..1000u32 {
            assert_eq!(roll_weapon_mod_mask(0, FormId(0x3000 + i), weapon, 0, 50, 0), 0);
        }

        // Single-slot weapon only yields bit 2 (slot 2) or 0.
        for i in 0..2000u32 {
            let m = roll_weapon_mod_mask(0, FormId(0x3000 + i), weapon, 0, 30, 0b010);
            assert!(m == 0 || m == 0b010, "unexpected mask {m:#b}");
        }

        // Two-slot weapon (slots 1 and 3 -> 0b101): only subset of 0b101; 1 mod > 2 mods > 0.
        let mut two_c1 = 0u32;
        let mut two_c2 = 0u32;
        for i in 0..samples {
            let m = roll_weapon_mod_mask(7, FormId(0x4000 + i), weapon, 0, 30, 0b101);
            assert_eq!(m & !0b101, 0, "invalid slot bit in {m:#b}");
            match m.count_ones() {
                0 => {}
                1 => two_c1 += 1,
                2 => two_c2 += 1,
                other => panic!("impossible count {other}"),
            }
        }
        let two_total = two_c1 + two_c2;
        assert!((11_500..=12_500).contains(&two_total), "level 30 expected ~60% modded, got {two_total}/{samples}");
        assert!(two_c1 > two_c2 && two_c2 > 0, "expected 1-mod ({two_c1}) > 2-mod ({two_c2}) > 0");

        // Three-slot weapon at level 1 (10%) vs level 30 (60%): strictly monotonic and
        // 1-mod > 2-mod > 3-mod > 0.
        let mut l1_modded = 0u32;
        for i in 0..samples {
            if roll_weapon_mod_mask(11, FormId(0x5000 + i), weapon, 0, 1, 0b111) != 0 {
                l1_modded += 1;
            }
        }
        assert!((1_700..=2_300).contains(&l1_modded), "level 1 expected ~10% modded, got {l1_modded}/{samples}");

        let mut c1 = 0u32;
        let mut c2 = 0u32;
        let mut c3 = 0u32;
        for i in 0..samples {
            let m = roll_weapon_mod_mask(11, FormId(0x5000 + i), weapon, 0, 30, 0b111);
            assert_eq!(m & !0b111, 0);
            match m.count_ones() {
                0 => {}
                1 => c1 += 1,
                2 => c2 += 1,
                3 => c3 += 1,
                other => panic!("impossible count {other}"),
            }
        }
        let l30_modded = c1 + c2 + c3;
        assert!((11_500..=12_500).contains(&l30_modded), "level 30 expected ~60% modded, got {l30_modded}/{samples}");
        assert!(c1 > c2 && c2 > c3 && c3 > 0, "expected 1-mod ({c1}) > 2-mod ({c2}) > 3-mod ({c3}) > 0");
    }

    #[test]
    fn off_by_default_leaves_npc_weapons_untouched() {
        let mut bytes = tes4();
        bytes.extend(top(b"IMOD", &[imod(0x101, "M1"), imod(0x102, "M2"), imod(0x103, "M3")].concat()));
        bytes.extend(top(b"WEAP", &weap(0x201, "ModGun", 25, [Some(0x101), Some(0x102), Some(0x103)])));
        let mut npcs = Vec::new();
        let mut refs = Vec::new();
        for i in 0..24u32 {
            npcs.extend(npc(0x300 + i, &format!("Npc{i}"), 30, &[(0x201, 1)]));
            refs.push(achr(0x400 + i, 0x300 + i));
        }
        bytes.extend(top(b"NPC_", &npcs));
        bytes.extend(top(b"CELL", &cell_with(0x500, &refs)));

        let f = Fixture::build(&bytes);
        let mut g = f.game(42);
        assert!(!g.tweaks.npc_weapon_mods, "must default to off");
        g.set_loaded(BTreeSet::from([FormId(0x500)]));

        for i in 0..24u32 {
            let r = FormId(0x400 + i);
            let inv = g.world.peek(r).and_then(|s| s.inventory.as_ref()).expect("outfitted inventory");
            assert!(inv.is_equipped(FormId(0x201)), "NPC main weapon must be equipped");
            assert_eq!(inv.equipped_mods(FormId(0x201)), 0, "off by default must attach no mods");
            assert!(g.extra.get(&r).is_none_or(|e| !e.npc_weapon_mods_rolled), "off by default must not mark rolled");
        }
    }

    #[test]
    fn on_attaches_valid_mods_idempotently_and_preserves_existing_and_ineligible() {
        let mut bytes = tes4();
        bytes.extend(top(b"IMOD", &[imod(0x101, "M1"), imod(0x103, "M3")].concat()));
        bytes.extend(top(
            b"WEAP",
            &[
                weap(0x201, "TwoSlotGun", 25, [Some(0x101), None, Some(0x103)]),
                weap(0x202, "PlainGun", 25, [None, None, None]),
            ]
            .concat(),
        ));
        let mut npcs = Vec::new();
        let mut refs = Vec::new();
        // Player base form (0x7) so PLAYER_REF has an inventory in the synthetic plugin.
        npcs.extend(npc(FormId::PLAYER_BASE.0, "Player", 30, &[]));
        // 32 high-level NPCs with two-slot moddable gun (slots 1 & 3 -> mask 0b101).
        for i in 0..32u32 {
            npcs.extend(npc(0x300 + i, &format!("Raider{i}"), 30, &[(0x201, 1)]));
            refs.push(achr(0x400 + i, 0x300 + i));
        }
        // NPC with non-moddable gun.
        npcs.extend(npc(0x350, "PlainNpc", 30, &[(0x202, 1)]));
        refs.push(achr(0x450, 0x350));
        // NPC whose weapon is pre-modded before option runs.
        npcs.extend(npc(0x351, "PreModNpc", 30, &[(0x201, 1)]));
        refs.push(achr(0x451, 0x351));
        // Leveled NPC (`LVLN`) resolving to an `NPC_` with moddable gun.
        npcs.extend(npc(0x360, "LeveledTemplate", 30, &[(0x201, 1)]));
        bytes.extend(top(b"NPC_", &npcs));
        bytes.extend(top(b"LVLN", &lvln(0x370, "LvlRaider", 0x360)));
        // Creature carrying a moddable weapon: must NOT be modified (`CREA`).
        bytes.extend(top(b"CREA", &crea(0x380, "Mutant", 30, &[(0x201, 1)])));
        refs.push(achr(0x480, 0x380));
        // Add several leveled NPC placements so at least one rolls a mod.
        for i in 0..12u32 {
            refs.push(achr(0x470 + i, 0x370));
        }
        bytes.extend(top(b"CELL", &cell_with(0x500, &refs)));

        let f = Fixture::build(&bytes);
        let mut g = f.game(123);
        // Pre-outfit and pre-mod 0x451 with slot 1 (bit 1) while option is off.
        g.world.outfit(FormId(0x451)).unwrap();
        {
            let inv = g.world.inventory_mut(FormId(0x451)).unwrap();
            let eq = inv.groups().find(|(id, e, _)| *id == FormId(0x201) && e.equipped).unwrap().1;
            assert!(inv.set_mods_one(FormId(0x201), eq, 0b001));
        }
        // Give player a moddable weapon and equip it too: player must never be touched.
        g.world.add_item_extra(FormId::PLAYER_REF, FormId(0x201), 1, Extra::default()).unwrap();
        g.world.equip(FormId::PLAYER_REF, FormId(0x201)).unwrap();

        g.tweaks.npc_weapon_mods = true;
        g.set_loaded(BTreeSet::from([FormId(0x500)]));

        let mut modded_count = 0usize;
        let mut snapshot = Vec::new();
        for i in 0..32u32 {
            let r = FormId(0x400 + i);
            let m = g.world.peek(r).and_then(|s| s.inventory.as_ref()).map_or(0, |inv| inv.equipped_mods(FormId(0x201)));
            assert_eq!(m & !0b101, 0, "only valid slots 1 and 3 allowed, got {m:#b}");
            // Live JIP query GetEquippedWeaponModFlags observes the exact same mask.
            let jip = g
                .run_query(
                    "Begin Function {}\nSetFunctionValue (GetEquippedWeaponModFlags)\nEnd\n",
                    crate::game::RunCtx { self_ref: Some(r), ..Default::default() },
                )
                .unwrap();
            assert_eq!(jip, XVal::Num(f64::from(m)), "GetEquippedWeaponModFlags must match inventory");
            if m != 0 {
                modded_count += 1;
            }
            snapshot.push(m);
        }
        assert!(modded_count > 5 && modded_count < 32, "expected mixed ~60% modded across 32 lvl-30 NPCs, got {modded_count}");

        // Non-moddable weapon stays 0.
        assert_eq!(g.world.peek(FormId(0x450)).unwrap().inventory.as_ref().unwrap().equipped_mods(FormId(0x202)), 0);
        // Pre-modded NPC weapon keeps its original mask (never overwritten).
        assert_eq!(g.world.peek(FormId(0x451)).unwrap().inventory.as_ref().unwrap().equipped_mods(FormId(0x201)), 0b001);
        // Creature stays 0.
        assert_eq!(g.world.peek(FormId(0x480)).unwrap().inventory.as_ref().unwrap().equipped_mods(FormId(0x201)), 0);
        // Player stays 0.
        assert_eq!(g.maybe_apply_npc_weapon_mods(FormId::PLAYER_REF), 0);
        assert_eq!(g.world.peek(FormId::PLAYER_REF).unwrap().inventory.as_ref().unwrap().equipped_mods(FormId(0x201)), 0);
        // Placed leveled NPCs (`LVLN`) ARE eligible and get valid mods.
        let lvl_modded = (0..12u32)
            .filter(|&i| {
                let m = g.world.peek(FormId(0x470 + i)).and_then(|s| s.inventory.as_ref()).map_or(0, |inv| inv.equipped_mods(FormId(0x201)));
                assert_eq!(m & !0b101, 0);
                m != 0
            })
            .count();
        assert!(lvl_modded > 0, "leveled LVLN NPCs must receive weapon mods");

        // Idempotent across repeated ticks/calls: zero re-rolling.
        for _ in 0..5 {
            g.outfit_loaded_actors();
            for i in 0..32u32 {
                assert_eq!(g.maybe_apply_npc_weapon_mods(FormId(0x400 + i)), 0);
            }
        }
        let after: Vec<u8> = (0..32u32)
            .map(|i| g.world.peek(FormId(0x400 + i)).unwrap().inventory.as_ref().unwrap().equipped_mods(FormId(0x201)))
            .collect();
        assert_eq!(snapshot, after, "weapon mods must never re-roll across ticks");

        // Session SetWeaponItemMod clearing WMI1/WMI3 leaves 0 available slots.
        g.field_overrides.insert((FormId(0x201), "WMI1/".into()), XVal::Ref(0));
        g.field_overrides.insert((FormId(0x201), "WMI3/".into()), XVal::Ref(0));
        assert_eq!(g.available_weapon_mod_mask(FormId(0x201)), 0);
    }
}
