//! JIP LN NVSE, batch 8 (#1006): actor base data, actor effects, ingestibles and base
//! effects, weapon fields and mods, idles, body-part names and limb models, cells and
//! water [documented: each page in the wiki export; docs/spec-notes/jip-records-8.md].
//! Setters change the form for the session, read by this file's getters and the earlier
//! ones (GetBodyPartName, GetBodyPartData, GetSkeletonModel, GetBaseEffectFlag,
//! GetEffectFlag, GetCellFlag, GetCellWaterForm, GetWaterFormEffect, GetWeaponItemMod…).

use crate::game::{Game, RunCtx};
use crate::nvse::{field_number, F32, U8};
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use serde_json::Value as Json;
use std::collections::HashMap;

/// What this batch keeps for the session (nothing is saved [hypothesis]; the
/// SetExcludedCombatActions page says its effect "is reset upon loading a game"
/// [documented]).
#[derive(Debug, Default)]
pub struct Jip8State {
    /// SetExcludedCombatActions' masks, by actor reference.
    pub excluded_combat: HashMap<FormId, u32>,
    /// SetBaseEffectFlag's flags, by base effect (`Game::base_effect_flags`).
    pub base_effect_flags: HashMap<FormId, u32>,
    /// SetBaseEffectScript's scripts, by base effect.
    pub base_effect_scripts: HashMap<FormId, FormId>,
    /// AddBaseEffectListEffect's and RemoveBaseEffectListEffect's lists, by base actor
    /// (`Game::actor_effects`).
    pub actor_effects: HashMap<FormId, Vec<FormId>>,
    /// SetWaterFormEffect's effects, by water (`None`: none) (`Game::water_effect`).
    pub water_effects: HashMap<FormId, Option<FormId>>,
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k) {
        Some(XVal::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn json_form(v: Option<&Json>) -> Option<FormId> {
    v.and_then(Json::as_str).and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).filter(|&f| f != 0).map(FormId)
}

fn as_ref(v: Option<XVal>) -> Option<FormId> {
    match v? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(r)),
        _ => None,
    }
}

/// ACBS flag bit 7, PC Level Mult [the bit: xEdit NPC_ and CREA].
const PC_LEVEL_MULT: u64 = 1 << 7;

/// The ACBS leveling fields 0-2 [documented: Get/SetActorLevelingData's table: 0 fixed
/// level or level mult, 1 calc min, 2 calc max; the fields: xEdit ACBS].
const LEVELING: [&str; 3] = ["level_or_level_mult_raw", "calc_min", "calc_max"];

/// The water traits 2-50 in WATR DNAM's order [documented: GetWaterTrait's table, which
/// follows the record: 2-5 unknown, 6 sun power … 50 layer 3 amplitude; the fields: xEdit
/// WATR, the schema's `WaterVisual`]. 9 and 15 are unused bytes and read 0; 12-14 are
/// colours. The original reads 15 as a value the record does not hold (6.53e-39, bits
/// 0x00471CFE, where the bytes are 0) [observed: jip8], something of its own at run
/// time; NVForever keeps the record's 0 [decision: no record value to give].
const WATER_TRAITS: [&str; 49] = [
    "unknown_0",
    "unknown_1",
    "unknown_2",
    "unknown_3",
    "sun_power",
    "reflectivity_amount",
    "fresnel_amount",
    "",
    "above_fog_near",
    "above_fog_far",
    "shallow_color",
    "deep_color",
    "reflection_color",
    "",
    "rain_force",
    "rain_velocity",
    "rain_falloff",
    "rain_dampner",
    "displacement_starting_size",
    "displacement_force",
    "displacement_velocity",
    "displacement_falloff",
    "displacement_dampner",
    "rain_starting_size",
    "noise_normals_scale",
    "noise_layer1_wind_direction",
    "noise_layer2_wind_direction",
    "noise_layer3_wind_direction",
    "noise_layer1_wind_speed",
    "noise_layer2_wind_speed",
    "noise_layer3_wind_speed",
    "noise_depth_falloff_start",
    "noise_depth_falloff_end",
    "above_fog_amount",
    "noise_normals_uv_scale",
    "under_fog_amount",
    "under_fog_near",
    "under_fog_far",
    "distortion_amount",
    "shininess",
    "reflection_hdr_multiplier",
    "light_radius",
    "light_brightness",
    "noise_layer1_uv_scale",
    "noise_layer2_uv_scale",
    "noise_layer3_uv_scale",
    "noise_layer1_amplitude",
    "noise_layer2_amplitude",
    "noise_layer3_amplitude",
];

/// The first condition actor value, PerceptionCondition (25); the embedded weapon's
/// actor value (WEAP DNAM, 0 Perception … 6 Brain) counts from it [the enum: xEdit WEAP;
/// documented: SetEmbeddedWeaponAV's example, BrainCondition].
const PERCEPTION_CONDITION: u16 = 25;

impl Game<'_> {
    /// A base actor's actor effects (SPLO), the record's last first [observed: jip8,
    /// the Super Mutant's two in reverse record order], AddBaseEffectListEffect's and
    /// RemoveBaseEffectListEffect's changes first.
    pub fn actor_effects(&self, base: FormId) -> Vec<FormId> {
        if let Some(v) = self.other.jip8.actor_effects.get(&base) {
            return v.clone();
        }
        let mut v: Vec<FormId> = self.data.record_all(base, "SPLO").into_iter().filter_map(|v| json_form(Some(v))).collect();
        v.reverse();
        v
    }

    /// A base effect's flags (MGEF DATA), SetBaseEffectFlag's change first; follows
    /// `TempCloneForm` clones while keeping post-clone edits on the original isolated.
    pub fn base_effect_flags(&self, mgef: FormId) -> u32 {
        if let Some(&f) = self.other.jip8.base_effect_flags.get(&mgef) {
            return f;
        }
        let base = self.unclone(mgef);
        self.data.magic_effects.get(&base).map_or(0, |m| m.flags)
    }

    /// A script-type base effect's associated script (MGEF DATA), SetBaseEffectScript's
    /// change first; `None` for non-script base effects or non-MGEF forms.
    pub fn base_effect_script(&self, mgef: FormId) -> Option<FormId> {
        let base = self.unclone(mgef);
        let def = self.data.magic_effects.get(&base)?;
        if def.archetype != crate::data::ARCHETYPE_SCRIPT {
            return None;
        }
        if let Some(&s) = self.other.jip8.base_effect_scripts.get(&mgef) {
            return Some(s);
        }
        def.script
    }

    /// Canonical session view of a base effect (MGEF), overlaying live
    /// `SetBaseEffectFlag` and `SetBaseEffectScript` changes and `TempCloneForm` clones.
    pub fn magic_effect_def(&self, mgef: FormId) -> Option<crate::data::MagicEffectDef> {
        let base = self.unclone(mgef);
        let mut def = self.data.magic_effects.get(&base)?.clone();
        def.flags = self.base_effect_flags(mgef);
        def.script = self.base_effect_script(mgef);
        Some(def)
    }

    /// Canonical session view of an ingestible (ALCH), overlaying live `ENIT` flags
    /// (`SetIngestibleFlag`, `SetEffectFlag`), `addiction_chance` (`SetAddictionChance`),
    /// `withdrawal_effect` (`SetAddictionEffect`), `sound_consume`
    /// (`SetIngestibleConsumeSound`), `value`, and `ETYP` (`SetEquipType`) changes.
    pub fn ingestible_def(&self, item: FormId) -> Option<crate::data::IngestibleDef> {
        let base = self.unclone(item);
        if self.data.sig(base) != Some(*b"ALCH") {
            return None;
        }
        let mut def = self.data.ingestibles.get(&base).cloned().unwrap_or_else(|| crate::data::IngestibleDef {
            equip_type: self.data.record_field(base, "ETYP", "").and_then(Json::as_i64).unwrap_or(-1),
            ..crate::data::IngestibleDef::default()
        });
        if let Some(XVal::Num(f)) = self.field_overrides.get(&(item, "ENIT/flags".into())) {
            def.flags = *f as u8;
        }
        if let Some(XVal::Num(c)) = self.field_overrides.get(&(item, "ENIT/addiction_chance".into())) {
            def.addiction_chance = *c as f32;
        }
        if let Some(v) = self.field_overrides.get(&(item, "ENIT/withdrawal_effect".into())) {
            def.withdrawal = as_ref(Some(v.clone()));
        }
        if let Some(v) = self.field_overrides.get(&(item, "ENIT/sound_consume".into())) {
            def.consume_sound = as_ref(Some(v.clone()));
        }
        if let Some(XVal::Num(e)) = self.field_overrides.get(&(item, "ETYP/".into())) {
            def.equip_type = *e as i64;
        }
        if let Some(XVal::Num(v)) = self.field_overrides.get(&(item, "ENIT/value".into())) {
            def.value = *v as i32;
        }
        Some(def)
    }

    /// An actor effect's SPIT type (0 Actor Effect .. 10 Addiction), following
    /// `TempCloneForm` clones.
    pub fn spell_type(&self, spell: FormId) -> Option<i64> {
        let base = self.unclone(spell);
        self.data.spell_types.get(&base).copied()
    }

    /// An actor effect's SPIT flags byte, `SetEffectFlag`'s change first; follows
    /// `TempCloneForm` clones while keeping post-clone edits on the original isolated.
    pub fn spell_flags(&self, spell: FormId) -> u8 {
        let base = self.unclone(spell);
        if self.data.sig(base) != Some(*b"SPEL") {
            return 0;
        }
        if let Some(XVal::Num(f)) = self.field_overrides.get(&(spell, "SPIT/flags".into())) {
            return *f as u8;
        }
        self.data.spell_flags.get(&base).copied().unwrap_or(0)
    }

    /// Whether a magic item's area effects ignore line of sight: true for non-spells or
    /// spells with live SPIT bit 4 ("Area Effect Ignores LOS").
    pub fn spell_ignores_los(&self, item: FormId) -> bool {
        self.spell_type(item).is_none() || self.spell_flags(item) & crate::area_effects::SPELL_AREA_IGNORES_LOS != 0
    }

    /// The effect flags byte of a SPEL (SPIT), ALCH (ENIT) or ENCH (ENIT), live session
    /// changes first (`SetEffectFlag`, `SetIngestibleFlag`).
    pub fn effect_flags(&self, form: FormId) -> u8 {
        let base = self.unclone(form);
        match self.data.sig(base).as_ref() {
            Some(b"SPEL") => self.spell_flags(form),
            Some(b"ALCH") => self.ingestible_def(form).map_or(0, |d| d.flags),
            Some(b"ENCH") => match self.field_overrides.get(&(form, "ENIT/flags".into())) {
                Some(XVal::Num(f)) => *f as u8,
                _ => self.data.record_field(base, "ENIT", "flags").and_then(Json::as_f64).unwrap_or(0.0) as u8,
            },
            _ => 0,
        }
    }

    /// Snapshot live SPEL, ALCH, MGEF and ENCH traits onto a `TempCloneForm` clone so
    /// pre-clone edits carry over while post-clone edits on the source remain isolated.
    pub(crate) fn snapshot_magic_traits(&mut self, source: FormId, clone: FormId) {
        let base = self.unclone(source);
        match self.data.sig(base).as_ref() {
            Some(b"SPEL") => {
                let flags = self.spell_flags(source);
                self.field_overrides.insert((clone, "SPIT/flags".into()), XVal::Num(f64::from(flags)));
            }
            Some(b"ALCH") => {
                let def = self.ingestible_def(source).unwrap_or_default();
                self.field_overrides.insert((clone, "ENIT/flags".into()), XVal::Num(f64::from(def.flags)));
                self.field_overrides.insert((clone, "ENIT/addiction_chance".into()), XVal::Num(crate::nvse::record_float(f64::from(def.addiction_chance))));
                self.field_overrides.insert((clone, "ENIT/withdrawal_effect".into()), XVal::Ref(def.withdrawal.map_or(0, |f| f.0)));
                self.field_overrides.insert((clone, "ENIT/sound_consume".into()), XVal::Ref(def.consume_sound.map_or(0, |f| f.0)));
                self.field_overrides.insert((clone, "ENIT/value".into()), XVal::Num(f64::from(def.value)));
                if def.equip_type >= 0 {
                    self.field_overrides.insert((clone, "ETYP/".into()), XVal::Num(def.equip_type as f64));
                }
                let weight = self
                    .field_overrides
                    .get(&(source, "DATA/".into()))
                    .cloned()
                    .or_else(|| self.data.record_field(base, "DATA", "").and_then(Json::as_f64).map(|x| XVal::Num(crate::nvse::record_float(x))));
                if let Some(w) = weight {
                    self.field_overrides.insert((clone, "DATA/".into()), w);
                }
            }
            Some(b"MGEF") => {
                let flags = self.base_effect_flags(source);
                self.other.jip8.base_effect_flags.insert(clone, flags);
                if let Some(script) = self.base_effect_script(source) {
                    self.other.jip8.base_effect_scripts.insert(clone, script);
                }
            }
            Some(b"ENCH") => {
                let flags = self.effect_flags(source);
                self.field_overrides.insert((clone, "ENIT/flags".into()), XVal::Num(f64::from(flags)));
            }
            _ => {}
        }
    }

    /// A water's actor effect (XNAM), SetWaterFormEffect's change first.
    pub fn water_effect(&self, water: FormId) -> Option<FormId> {
        match self.other.jip8.water_effects.get(&water) {
            Some(&e) => e,
            None => self.data.water_effects.get(&water).copied(),
        }
    }

    /// The excluded combat actions of an actor (SetExcludedCombatActions), 0 until set.
    pub fn excluded_combat_actions(&self, actor: FormId) -> u32 {
        self.other.jip8.excluded_combat.get(&actor).copied().unwrap_or(0)
    }

    /// A field of the part of body-part data `bptd` whose type (BPND part_type) is
    /// `part`: its name (BPTN) or limb replacement model (NAM1), the session's change
    /// first; `None` when the record has no such part. A part's subrecords start with
    /// BPTN or BPNN [documented: xEdit BPTD, schema combat.json].
    pub fn body_part_text(&self, bptd: FormId, part: i64, sub: &str) -> Option<String> {
        if let Some(XVal::Str(s)) = self.field_overrides.get(&(bptd, format!("BPTD#{part}/{sub}"))) {
            // Only for a part the record has.
            return self.body_part_group(bptd, part).map(|_| s.clone());
        }
        let group = self.body_part_group(bptd, part)?;
        Some(group.iter().find(|(s, _)| s == sub.as_bytes()).and_then(|(_, v)| v.as_str()).unwrap_or_default().to_string())
    }

    /// The subrecords of one part of body-part data, BPTN (or BPNN) to the next.
    fn body_part_group(&self, bptd: FormId, part: i64) -> Option<Vec<([u8; 4], &Json)>> {
        let subs = self.data.record_fields.get(&bptd)?;
        let mut group: Vec<([u8; 4], &Json)> = Vec::new();
        let mut named = false;
        let mut found = false;
        for (sig, v) in subs {
            match sig {
                b"BPTN" => {
                    if found {
                        break;
                    }
                    group.clear();
                    named = true;
                }
                b"BPNN" if !named => {
                    if found {
                        break;
                    }
                    group.clear();
                }
                b"BPNN" => named = false,
                b"BPND" => found = v.get("part_type").and_then(Json::as_i64) == Some(part),
                _ => {}
            }
            group.push((*sig, v));
        }
        found.then_some(group)
    }

    fn set_field8(&mut self, f: FormId, sub: &str, path: &str, v: XVal) {
        self.field_overrides.insert((f, format!("{sub}/{path}")), v);
    }

    fn num_field8(&self, f: FormId, sub: &str, path: &str) -> f64 {
        crate::nvse::record_value(self, f, sub, path).and_then(|v| v.num().ok()).unwrap_or(0.0)
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let of_sig = |g: &Game, f: Option<FormId>, sig: &[u8; 4]| f.filter(|&f| g.data.sig(f) == Some(*sig));
    let is_actor = |g: &Game, f: FormId| matches!(g.data.sig(f).as_ref(), Some(b"NPC_" | b"CREA"));
    // `[baseActor]` at `k`, else the calling reference's base [documented: the pages'
    // `ref.F` and `F … baseForm` forms].
    let actor_at = |g: &Game, k: usize| form(args, k).or(subject).map(|f| base(g, f)).filter(|&a| is_actor(g, a));
    let zero = XVal::Num(0.0);
    // The setters of a field return 0 [hypothesis: as the record setters of batch 5 were
    // observed to, though the catalogue names a result, jip-records-5.md].
    let v = match name {
        // ---- actor base data
        "getdeathitem" => XVal::Ref(actor_at(g, 0).and_then(|a| as_ref(crate::nvse::record_value(g, a, "INAM", ""))).map_or(0, |f| f.0)),
        "setdeathitem" => {
            if let Some(a) = actor_at(g, 1) {
                g.set_field8(a, "INAM", "", XVal::Ref(form(args, 0).map_or(0, |f| f.0)));
            }
            zero
        }
        "getactortemplate" => XVal::Ref(actor_at(g, 0).and_then(|a| as_ref(crate::nvse::record_value(g, a, "TPLT", ""))).map_or(0, |f| f.0)),
        // `type [base]`: 0 the fixed level, or the level mult when the actor has PC Level
        // Mult (stored × 1000), 1 calc min, 2 calc max [documented: the page's table and
        // example, "level multiplier to 0.75"; the ×1000: xEdit ACBS].
        "getactorlevelingdata" => {
            let t = num(args, 0, -1.0) as i64;
            let (Some(a), Some(path)) = (actor_at(g, 1), usize::try_from(t).ok().and_then(|t| LEVELING.get(t))) else { return Some(Ok(zero)) };
            let v = g.num_field8(a, "ACBS", path);
            let mult = t == 0 && g.num_field8(a, "ACBS", "flags") as u64 & PC_LEVEL_MULT != 0;
            XVal::Num(if mult { v / 1000.0 } else { v })
        }
        "setactorlevelingdata" => {
            let t = num(args, 0, -1.0) as i64;
            if let (Some(a), Some(path)) = (actor_at(g, 2), usize::try_from(t).ok().and_then(|t| LEVELING.get(t))) {
                let mult = t == 0 && g.num_field8(a, "ACBS", "flags") as u64 & PC_LEVEL_MULT != 0;
                let v = num(args, 1, f64::NAN) * if mult { 1000.0 } else { 1.0 };
                // u16 fields; calc max not below calc min [observed: jip8, Angela's calc
                // max set to 0 reads her calc min, 1].
                if let Some(v) = field_number(v.round(), 0.0, 65535.0) {
                    let v = if t == 2 { v.max(g.num_field8(a, "ACBS", "calc_min")) } else { v };
                    g.set_field8(a, "ACBS", path, XVal::Num(v));
                }
            }
            zero
        }
        // `faction [base]`: the rank, -1 when not a member [documented]; the factions
        // of the base, or of its template when it takes its factions from one
        // [hypothesis: as GetBaseFactions].
        "getbasefactionrank" => {
            let (Some(f), Some(a)) = (form(args, 0), actor_at(g, 1)) else { return Some(Ok(XVal::Num(-1.0))) };
            let src = g.data.base_template_source(a, |d| d.template_factions);
            // SetBaseFactionRank's first (`crate::nvse_jip11`).
            if let Some(r) = g.base_rank_override(src, f) {
                return Some(Ok(XVal::Num(f64::from(r))));
            }
            XVal::Num(g.data.actors.get(&src).and_then(|d| d.factions.iter().find(|(x, _)| *x == f)).map_or(-1.0, |&(_, r)| f64::from(r)))
        }
        // `ref.GetInFactionList list`: a member of one of the list's factions, by the
        // actor's factions now [documented].
        "getinfactionlist" => {
            let (Some(r), Some(l)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            yes(crate::functions::refs::list_members(g, l).into_iter().any(|f| crate::functions::actor::faction_rank(g, r, f) >= 0))
        }
        // `[base]`: the SPLO list [documented: the pages; the field: xEdit NPC_/CREA].
        "getnumactoreffects" => XVal::Num(actor_at(g, 0).map_or(0, |a| g.actor_effects(a).len()) as f64),
        "getnthactoreffect" => {
            let i = num(args, 0, -1.0);
            let all = actor_at(g, 1).map(|a| g.actor_effects(a)).unwrap_or_default();
            XVal::Ref(if i >= 0.0 { all.get(i as usize).map_or(0, |f| f.0) } else { 0 })
        }
        // `effect [base]`: only an actor effect (SPEL) [documented: "the specified Actor
        // Effect"]; added at the front, once [observed: jip8, after two adds the list is
        // one longer and the old first is second].
        "addbaseeffectlisteffect" | "removebaseeffectlisteffect" => {
            let (Some(e), Some(a)) = (of_sig(g, form(args, 0), b"SPEL"), actor_at(g, 1)) else { return Some(Ok(zero)) };
            let mut list = g.actor_effects(a);
            if name == "addbaseeffectlisteffect" {
                if !list.contains(&e) {
                    list.insert(0, e);
                }
            } else {
                list.retain(|&x| x != e);
            }
            g.other.jip8.actor_effects.insert(a, list);
            zero
        }
        // `creature bpd`: CREA PNAM [documented: SetBodyPartData; the field: xEdit CREA].
        "setbodypartdata" => {
            if let (Some(c), Some(b)) = (of_sig(g, form(args, 0), b"CREA"), of_sig(g, form(args, 1), b"BPTD")) {
                g.set_field8(c, "PNAM", "", XVal::Ref(b.0));
            }
            zero
        }
        // `creature path`; `npc [path]`, left out the default skeleton of the NPC's sex
        // (sNPCModelMale, sNPCModelFemale) [documented: the pages]. MODL.
        "setcreskeletonmodel" => {
            if let (Some(c), Some(p)) = (of_sig(g, form(args, 0), b"CREA"), text(args, 1)) {
                g.set_field8(c, "MODL", "", XVal::Str(p));
            }
            zero
        }
        "setnpcskeletonmodel" => {
            if let Some(n) = of_sig(g, form(args, 0), b"NPC_") {
                let female = g.num_field8(n, "ACBS", "flags") as u64 & 1 != 0;
                let dflt = || match g.world.setting_value(if female { "sNPCModelFemale" } else { "sNPCModelMale" }) {
                    Some(fnv_sim::settings::Setting::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let p = text(args, 1).unwrap_or_else(dflt);
                g.set_field8(n, "MODL", "", XVal::Str(p));
            }
            zero
        }
        // `actorRef.F [mask]`: the 27 combat action bits [documented: the page's table].
        "getexcludedcombatactions" => XVal::Num(subject.map_or(0, |r| g.excluded_combat_actions(r)) as f64),
        "setexcludedcombatactions" => {
            if let (Some(r), Some(m)) = (subject, field_number(num(args, 0, f64::NAN), 0.0, f64::from(u32::MAX))) {
                let m = m as u32 & ((1 << 27) - 1);
                if m == 0 {
                    g.other.jip8.excluded_combat.remove(&r);
                } else {
                    g.other.jip8.excluded_combat.insert(r, m);
                }
            }
            zero
        }
        // ---- ingestibles and base effects
        // ALCH ENIT: the addiction chance and the addiction (withdrawal) effect
        // [documented: the pages; the fields: xEdit ALCH].
        "getaddictionchance" => XVal::Num(form(args, 0).and_then(|a| g.ingestible_def(a)).map_or(0.0, |d| crate::nvse::record_float(f64::from(d.addiction_chance)))),
        "setaddictionchance" => {
            // Kept as given, 50, 1.5 and -0.5 too [observed: jip8]; an f32 field.
            let alch = form(args, 0).filter(|&a| g.data.sig(g.unclone(a)) == Some(*b"ALCH"));
            if let (Some(a), Some(c)) = (alch, field_number(num(args, 1, f64::NAN), F32.0, F32.1)) {
                g.set_field8(a, "ENIT", "addiction_chance", XVal::Num(c));
            }
            zero
        }
        "getaddictioneffect" => XVal::Ref(form(args, 0).and_then(|a| g.ingestible_def(a)).and_then(|d| d.withdrawal).map_or(0, |f| f.0)),
        "setaddictioneffect" => {
            // An actor effect; none is ignored [documented: "actorEffect"; observed: jip8,
            // none leaves Cateye's effect].
            let alch = form(args, 0).filter(|&a| g.data.sig(g.unclone(a)) == Some(*b"ALCH"));
            let spel = form(args, 1).filter(|&e| g.data.sig(g.unclone(e)) == Some(*b"SPEL"));
            if let (Some(a), Some(e)) = (alch, spel) {
                g.set_field8(a, "ENIT", "withdrawal_effect", XVal::Ref(e.0));
            }
            zero
        }
        // A script-type (archetype 1) base effect's script [documented: the pages].
        "getbaseeffectscript" => XVal::Ref(form(args, 0).and_then(|f| g.base_effect_script(f)).map_or(0, |s| s.0)),
        "setbaseeffectscript" => {
            let m = form(args, 0).filter(|&f| g.data.magic_effects.get(&g.unclone(f)).is_some_and(|m| m.archetype == crate::data::ARCHETYPE_SCRIPT));
            if let (Some(m), Some(s)) = (m, form(args, 1).filter(|&s| g.data.sig(g.unclone(s)) == Some(*b"SCPT"))) {
                g.other.jip8.base_effect_scripts.insert(m, s);
            }
            zero
        }
        // `effect flag set`: any of DATA's 32 bits, not only the page's table's
        // [documented: the table; observed: jip8, flag 11 is set].
        "setbaseeffectflag" => {
            let k = num(args, 1, -1.0);
            let m = form(args, 0).filter(|&f| g.data.magic_effects.contains_key(&g.unclone(f)));
            if let Some(m) = m.filter(|_| k.fract() == 0.0 && (0.0..32.0).contains(&k)) {
                let flags = g.base_effect_flags(m);
                let bit = 1u32 << (k as u32);
                let new = if num(args, 2, 0.0) != 0.0 { flags | bit } else { flags & !bit };
                g.other.jip8.base_effect_flags.insert(m, new);
            }
            zero
        }
        // `effect flag set`: any of the flags byte's bits 0-7, not only the page's tables'
        // [documented: the tables; observed: jip8, every bit tried sticks on all three];
        // SPEL SPIT, ENCH and ALCH ENIT [the fields: xEdit].
        "seteffectflag" => {
            let k = num(args, 1, -1.0);
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let sub = match g.data.sig(g.unclone(f)).as_ref() {
                Some(b"SPEL") => "SPIT",
                Some(b"ENCH" | b"ALCH") => "ENIT",
                _ => return Some(Ok(zero)),
            };
            if (0.0..8.0).contains(&k) && k.fract() == 0.0 {
                let flags = u64::from(g.effect_flags(f));
                let bit = 1u64 << (k as u32);
                let new = if num(args, 2, 0.0) != 0.0 { flags | bit } else { flags & !bit };
                g.set_field8(f, sub, "flags", XVal::Num(new as f64));
            }
            zero
        }
        // ---- weapons
        // VNAM: 0 loud, 1 normal, 2 silent [documented: the pages; the field: xEdit WEAP].
        "getweapondetectionsoundlevel" => XVal::Num(of_sig(g, form(args, 0), b"WEAP").map_or(0.0, |w| g.num_field8(w, "VNAM", ""))),
        "setweapondetectionsoundlevel" => {
            if let (Some(w), Some(v)) = (of_sig(g, form(args, 0), b"WEAP"), field_number(num(args, 1, f64::NAN), 0.0, 2.0)) {
                g.set_field8(w, "VNAM", "", XVal::Num(v.trunc()));
            }
            zero
        }
        // `weapon slot [mod]`: WMI1-3; left out, null, or a non-IMOD form clears the slot
        // to 0 [documented: the page's examples; observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23, GSDocMitchellHouse (0x00103DF9),
        // 10 official base masters, `jip_20_weapon_item_mod_non_imod`
        // (`tools/capture/nvse/catalogs/wave4a_extenders.json`):
        // `PROBE|jip_20_weapon_item_mod_non_imod|m0=0010C858,m1=00000000,eq0=1,eqSt=0,eqM0=0`;
        // no DLL or source consulted].
        "setweaponitemmod" => {
            let slot = num(args, 1, 0.0);
            if let Some(w) = of_sig(g, form(args, 0), b"WEAP").filter(|_| (1.0..=3.0).contains(&slot)) {
                let m = form(args, 2).filter(|&m| g.data.sig(g.unclone(m)) == Some(*b"IMOD"));
                g.set_field8(w, &format!("WMI{}", slot as u8), "", XVal::Ref(m.map_or(0, |m| m.0)));
            }
            zero
        }
        // `weapon slot effect`: DNAM effect_mod1-3, 1-16; 0 (none) is ignored [documented:
        // the page's table, 0-14, and the JohnnyGuitar note; observed: jip8, 0 leaves the
        // effect and 15 is stored; hypothesis: 16, the enum's last].
        "setweaponitemmodeffect" | "setweaponitemmodvalue" => {
            let slot = num(args, 1, 0.0);
            let Some(w) = of_sig(g, form(args, 0), b"WEAP").filter(|_| (1.0..=3.0).contains(&slot)) else { return Some(Ok(zero)) };
            let (path, range) = if name == "setweaponitemmodeffect" { ("effect_mod", (1.0, 16.0)) } else { ("value_a_mod", F32) };
            let raw = num(args, 2, f64::NAN);
            if range != F32 && !(1.0..=16.0).contains(&raw) {
                return Some(Ok(zero));
            }
            if let Some(v) = field_number(raw, range.0, range.1) {
                let v = if range == F32 { v } else { v.trunc() };
                g.set_field8(w, "DNAM", &format!("{path}{}", slot as u8), XVal::Num(v));
            }
            zero
        }
        // DNAM on_hit 0-3 [documented: the page's table].
        "setweapononhitsetting" => {
            if let (Some(w), Some(v)) = (of_sig(g, form(args, 0), b"WEAP"), field_number(num(args, 1, f64::NAN), 0.0, 3.0)) {
                g.set_field8(w, "DNAM", "on_hit", XVal::Num(v.trunc()));
            }
            zero
        }
        // `weapon setMax delay`: DNAM's semi-automatic fire delay min or max; stores raw
        // finite floats including negative values without clamping [observed: Fallout: New
        // Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, probe jip_22_weapon_semiauto_delay_neg:
        // 0.000 -> -0.500].
        "setweaponsemiautofiredelay" => {
            let path = if num(args, 1, 0.0) != 0.0 { "semi_automatic_fire_delay_max" } else { "semi_automatic_fire_delay_min" };
            if let (Some(w), Some(v)) = (of_sig(g, form(args, 0), b"WEAP"), field_number(num(args, 2, f64::NAN), F32.0, F32.1)) {
                g.set_field8(w, "DNAM", path, XVal::Num(v));
            }
            zero
        }
        // `weapon mask path`: 0 MODL, 1-7 MWD1-7, as GetWeaponModel reads them
        // [documented: the page's table].
        "setweaponmodel" => {
            let mask = num(args, 1, -1.0);
            if let (Some(w), Some(p)) = (of_sig(g, form(args, 0), b"WEAP").filter(|_| (0.0..=7.0).contains(&mask)), text(args, 2)) {
                let sub = if mask as u8 == 0 { "MODL".to_string() } else { format!("MWD{}", mask as u8) };
                g.set_field8(w, &sub, "", XVal::Str(p));
            }
            zero
        }
        // DNAM reload_animation_mod, 0-22 (ReloadA … ReloadZ) [the enum: xEdit WEAP].
        "getweaponmodreloadanim" => XVal::Num(of_sig(g, form(args, 0), b"WEAP").map_or(0.0, |w| g.num_field8(w, "DNAM", "reload_animation_mod"))),
        "setweaponmodreloadanim" => {
            if let (Some(w), Some(v)) = (of_sig(g, form(args, 0), b"WEAP"), field_number(num(args, 1, f64::NAN), 0.0, 22.0)) {
                g.set_field8(w, "DNAM", "reload_animation_mod", XVal::Num(v.trunc()));
            }
            zero
        }
        // `weapon type`: the slot (1-3) whose mod effect is the type, else 0; a placed
        // weapon by its base [documented: the page]. The type IDs are DNAM's mod effect
        // values [the enum: xEdit WEAP, 5 and 6 the two regenerations, 16 suppressor].
        "weaponhasmodtype" => {
            let t = num(args, 1, 0.0);
            let w = of_sig(g, form(args, 0).map(|f| base(g, f)), b"WEAP");
            XVal::Num(w.filter(|_| t >= 1.0).and_then(|w| (1..=3u8).find(|k| g.num_field8(w, "DNAM", &format!("effect_mod{k}")) == t.trunc())).map_or(0.0, f64::from))
        }
        // `ref.F type`: one of the attached mods (the equipped weapon's mod bits 1, 2, 4)
        // has the type [documented].
        "equippedweaponhasmodtype" => {
            let t = num(args, 0, 0.0);
            let Some(a) = subject.filter(|_| t >= 1.0) else { return Some(Ok(zero)) };
            let Some(w) = crate::functions::items::equipped_weapon(g, a) else { return Some(Ok(zero)) };
            let bits = g.world.inventory_mut(a).map_or(0, |inv| inv.equipped_mods(w));
            yes((1..=3u8).any(|k| bits & (1 << (k - 1)) != 0 && g.num_field8(w, "DNAM", &format!("effect_mod{k}")) == t.trunc()))
        }
        // `weapon av`: the condition actor values PerceptionCondition … BrainCondition,
        // DNAM's 0-6 [documented: the page's example; the enum: xEdit WEAP].
        "setembeddedweaponav" => {
            let code = match args.get(1) {
                Some(XVal::Num(c)) => Some(*c as u16),
                Some(XVal::Str(n)) => crate::functions::av_code(g, n).ok(),
                _ => None,
            };
            if let (Some(w), Some(c)) = (of_sig(g, form(args, 0), b"WEAP"), code.filter(|c| (PERCEPTION_CONDITION..PERCEPTION_CONDITION + 7).contains(c))) {
                g.set_field8(w, "DNAM", "embedded_weapon_actor_value", XVal::Num(f64::from(c - PERCEPTION_CONDITION)));
            }
            zero
        }
        // NNAM, the embedded weapon node [documented: the page; the field: xEdit WEAP].
        "setembeddedweaponnode" => {
            if let (Some(w), Some(n)) = (of_sig(g, form(args, 0), b"WEAP"), text(args, 1)) {
                g.set_field8(w, "NNAM", "", XVal::Str(n));
            }
            zero
        }
        // ---- idles
        // IDLE DATA loop_min / loop_max (u8) [documented: the pages; the fields: xEdit
        // IDLE].
        "getidlelooptimes" => {
            let path = if num(args, 1, 0.0) != 0.0 { "loop_max" } else { "loop_min" };
            XVal::Num(of_sig(g, form(args, 0), b"IDLE").map_or(0.0, |i| g.num_field8(i, "DATA", path)))
        }
        "setidlelooptimes" => {
            // Each clamped to a u8, the max raised to the min [observed: jip8, 300 2 reads
            // 255 255].
            if let Some(i) = of_sig(g, form(args, 0), b"IDLE") {
                if let Some(v) = field_number(num(args, 1, f64::NAN), U8.0, U8.1) {
                    g.set_field8(i, "DATA", "loop_min", XVal::Num(v.trunc()));
                }
                if let Some(v) = field_number(num(args, 2, f64::NAN), U8.0, U8.1) {
                    let min = g.num_field8(i, "DATA", "loop_min");
                    g.set_field8(i, "DATA", "loop_max", XVal::Num(v.trunc().max(min)));
                }
            }
            zero
        }
        // ---- body-part data
        // `bpd part`: the part's NAM1 [documented: the page]; "" without one.
        "getbodypartreplacementmodel" => {
            let part = num(args, 1, -1.0) as i64;
            XVal::Str(of_sig(g, form(args, 0), b"BPTD").and_then(|b| g.body_part_text(b, part, "NAM1")).unwrap_or_default())
        }
        "setbodypartreplacementmodel" | "setbodypartname" => {
            let part = num(args, 1, -1.0) as i64;
            let sub = if name == "setbodypartname" { "BPTN" } else { "NAM1" };
            if let (Some(b), Some(t)) = (of_sig(g, form(args, 0), b"BPTD"), text(args, 2)) {
                if g.body_part_group(b, part).is_some() {
                    g.field_overrides.insert((b, format!("BPTD#{part}/{sub}")), XVal::Str(t));
                }
            }
            zero
        }
        // ---- cells and water
        // `cell flag set`: 0-7 DATA's bits (4 too, which the table leaves out), 8-16 the
        // lighting template inherit flags (LNAM bits 0-8) [documented: the page's table;
        // the bits: xEdit CELL; observed: jip8, 4 is set, and GetCellFlag 8-16 follow LNAM
        // in this order on Doc Mitchell's house and the saloon].
        "setcellflag" => {
            let k = num(args, 1, -1.0);
            let Some(c) = of_sig(g, form(args, 0), b"CELL").filter(|_| (0.0..=16.0).contains(&k) && k.fract() == 0.0) else { return Some(Ok(zero)) };
            let (sub, b) = if k < 8.0 { ("DATA", k as u32) } else { ("LNAM", k as u32 - 8) };
            let flags = g.num_field8(c, sub, "") as u64;
            let new = if num(args, 2, 0.0) != 0.0 { flags | 1 << b } else { flags & !(1 << b) };
            let prev = if k >= 8.0 { g.field_overrides.get(&(c, "LNAM/".into())).cloned() } else { None };
            let next = XVal::Num(new as f64);
            g.set_field8(c, sub, "", next);
            if k >= 8.0 && !matches!(prev, Some(XVal::Num(p)) if p.to_bits() == (new as f64).to_bits()) {
                g.bump_lighting_revision();
            }
            zero
        }
        // `cell water`: XCWT [documented: the page; the field: xEdit CELL].
        "setcellwaterform" => {
            if let (Some(c), Some(w)) = (of_sig(g, form(args, 0), b"CELL"), of_sig(g, form(args, 1), b"WATR")) {
                g.set_field8(c, "XCWT", "", XVal::Ref(w.0));
            }
            zero
        }
        // `water effect`: XNAM, an actor effect [documented: the page].
        "setwaterformeffect" => {
            let e = form(args, 1);
            if let Some(w) = of_sig(g, form(args, 0), b"WATR").filter(|_| e.is_none_or(|e| g.data.sig(e) == Some(*b"SPEL"))) {
                g.other.jip8.water_effects.insert(w, e);
            }
            zero
        }
        // `water trait`: 0 opacity (ANAM), 1 the flags (FNAM), 2-50 DNAM; a colour as
        // R × 1000000 + G × 1000 + B [documented: the page's table]; past them, and the
        // table's unused 9 and 15, 0.
        "getwatertrait" => {
            let Some(w) = of_sig(g, form(args, 0), b"WATR") else { return Some(Ok(zero)) };
            let t = num(args, 1, -1.0);
            if t.fract() != 0.0 || !(0.0..=50.0).contains(&t) {
                return Some(Ok(zero));
            }
            XVal::Num(match t as usize {
                0 => g.num_field8(w, "ANAM", ""),
                1 => g.num_field8(w, "FNAM", ""),
                t => match WATER_TRAITS[t - 2] {
                    "" => 0.0,
                    p if p.ends_with("_color") => {
                        let c = |k: &str| g.num_field8(w, "DNAM", &format!("{p}.{k}"));
                        c("red") * 1_000_000.0 + c("green") * 1000.0 + c("blue")
                    }
                    p => g.num_field8(w, "DNAM", p),
                },
            })
        }
        _ => return None,
    };
    Some(Ok(v))
}
