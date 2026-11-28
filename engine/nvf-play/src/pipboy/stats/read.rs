//! Reading what the STATS page shows from the game: the records' words and pictures
//! ([`Forms`], read once each from the plugins) and the player's figures
//! ([`snapshot`], from the scripts' game and the simulation's world).

use super::model::{self, Change, EffectLine, Hints, Row, RowKey, Snapshot, Standing, LIMBS};
use crate::script::ScriptState;
use crate::sim::SimState;
use fnv_esm::{LoadOrder, Schema};
use fnv_game::actor_values::{self as av, SKILLS, SPECIALS};
use fnv_game::functions as gf;
use fnv_game::game::ActiveEffect;
use fnv_sim::FormId;
use fnv_world::fields::{formid_of, global, winner, Fields};
use std::collections::HashMap;

/// A record's words and picture.
#[derive(Clone, Debug, Default)]
pub struct Text {
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    /// AVIF ANAM: the short name (`STR`, `Melee Weap.`) [observed: a zero-ended text in
    /// every AVIF; the EFF screenshot writes "Melee Weap." for Melee Weapons].
    pub short: Option<String>,
}

/// A perk's DATA: trait, minimum level, ranks, playable, hidden [hypothesis: the five
/// bytes as xEdit names them; `IntenseTraining`'s are 0, 2, 10, 1, 0 and it has ten
/// ranks]. Playable only says the level-up menu offers it [documented: GECK wiki
/// Category:Perk], so the page reads ranks and Hidden alone (#3118).
#[derive(Clone, Copy, Debug, Default)]
pub struct PerkData {
    pub ranks: u8,
    pub hidden: bool,
}

impl PerkData {
    /// From the DATA bytes; a 4-byte DATA (older records: Ant Sight) has no Hidden byte.
    pub fn of(d: &[u8]) -> PerkData {
        PerkData { ranks: d.get(2).copied().unwrap_or(1), hidden: d.get(4).is_some_and(|&b| b != 0) }
    }

    /// Whether the Perks list shows a perk the player has: unless it is Hidden, "does not
    /// show up in the user interface" [documented: GECK wiki Category:Perk]. Perks that
    /// are not Playable (challenge perks such as Lord Death, implants, companion and quest
    /// perks) are given by scripts and listed [hypothesis: as the original lists them;
    /// #3118].
    pub fn listed(&self) -> bool {
        !self.hidden
    }
}

/// An enchantment (ENCH): its name, whether it is worn apparel's, and its value changes
/// (actor value, magnitude, detrimental).
#[derive(Clone, Debug, Default)]
pub struct Ench {
    pub name: String,
    pub apparel: bool,
    pub changes: Vec<(u16, f64, bool)>,
}

/// The records read so far.
pub struct Forms {
    lo: &'static LoadOrder<'static>,
    schema: &'static Schema,
    texts: HashMap<u32, Text>,
    perks: HashMap<u32, PerkData>,
    enchs: HashMap<u32, Ench>,
    /// MGEF: (archetype is Value Modifier, flag Detrimental).
    effects: HashMap<u32, (bool, bool)>,
    /// An item's enchantment (EITM).
    item_ench: HashMap<u32, Option<u32>>,
    /// The hints' items from the DOBJ list ([`Forms::hint_items`]), read once.
    hint_items: Option<[Option<u32>; 4]>,
}

fn fields(lo: &'static LoadOrder<'static>, schema: &'static Schema, id: u32) -> Option<(usize, Fields<'static>)> {
    let (pi, rec) = winner(lo, id)?;
    Some((pi, Fields::new(rec, pi, schema).ok()?))
}

/// A zero-ended text in `page`, the load order's code page as `FULL` is read in: an
/// AVIF's short name (ANAM) on a Russian install is Windows-1251, and read byte for byte
/// as Latin-1 its letters had no glyphs and showed as `?` (#3400).
fn zstring(b: &[u8], page: fnv_util::text::CodePage) -> String {
    page.decode_z(b)
}

impl Forms {
    pub fn new(lo: &'static LoadOrder<'static>, schema: &'static Schema) -> Forms {
        Forms { lo, schema, texts: HashMap::new(), perks: HashMap::new(), enchs: HashMap::new(), effects: HashMap::new(), item_ench: HashMap::new(), hint_items: None }
    }

    /// FULL, DESC, ICON (and ANAM) of a record.
    pub fn text(&mut self, id: u32) -> &Text {
        let (lo, schema) = (self.lo, self.schema);
        self.texts.entry(id).or_insert_with(|| {
            let Some((_, f)) = fields(lo, schema, id) else { return Text::default() };
            Text {
                name: f.string(b"FULL").unwrap_or_default(),
                description: f.string(b"DESC").unwrap_or_default().trim().to_string(),
                icon: f.string(b"ICON").filter(|s| !s.trim().is_empty()),
                short: f.raw(b"ANAM").map(|s| zstring(&s.data, fnv_util::text::code_page())).filter(|s| !s.is_empty()),
            }
        })
    }

    /// The words of an actor value (its AVIF record).
    pub fn value(&mut self, code: u16) -> Text {
        let t = model::avif_form(code).map(|id| self.text(id).clone()).unwrap_or_default();
        if t.name.is_empty() {
            Text { name: gf::AV_NAMES.get(code as usize).map_or_else(String::new, |n| n.to_string()), ..t }
        } else {
            t
        }
    }

    pub fn perk(&mut self, id: u32) -> PerkData {
        let (lo, schema) = (self.lo, self.schema);
        *self.perks.entry(id).or_insert_with(|| {
            let d = fields(lo, schema, id).and_then(|(_, f)| f.raw(b"DATA").map(|s| s.data.to_vec())).unwrap_or_default();
            PerkData::of(&d)
        })
    }

    fn effect(&mut self, id: u32) -> (bool, bool) {
        let (lo, schema) = (self.lo, self.schema);
        *self.effects.entry(id).or_insert_with(|| {
            let Some(d) = fields(lo, schema, id).and_then(|(_, f)| f.get(b"DATA")) else { return (false, false) };
            let modifier = d.get("archetype").and_then(|a| a.as_str()) == Some("Value Modifier");
            let detrimental = d.get("flags").and_then(|f| f.get("set")).and_then(|s| s.as_array()).is_some_and(|s| s.iter().any(|x| x.as_str() == Some("Detrimental")));
            (modifier, detrimental)
        })
    }

    /// An enchantment's value changes: each EFID's base effect when it is a Value
    /// Modifier [hypothesis: only those change a value the page shows], with the EFIT's
    /// magnitude and actor value [documented: schema `common.json` EFIT].
    pub fn ench(&mut self, id: u32) -> Ench {
        if let Some(e) = self.enchs.get(&id) {
            return e.clone();
        }
        let mut out = Ench::default();
        if let Some((pi, f)) = fields(self.lo, self.schema, id) {
            out.name = f.string(b"FULL").unwrap_or_default();
            out.apparel = f.get(b"ENIT").and_then(|v| v.get("type").and_then(|t| t.as_str().map(|t| t == "Apparel"))).unwrap_or(false);
            let mut base = None;
            for s in &f.subs {
                match &s.sig {
                    b"EFID" => base = f.decode(s).as_ref().and_then(formid_of).and_then(|l| global(self.lo, pi, l)),
                    b"EFIT" => {
                        let (Some(b), Some(v)) = (base.take(), f.decode(s)) else { continue };
                        let code = v.get("actor_value").and_then(|a| a.as_str()).and_then(gf::av_by_name);
                        let magnitude = v.get("magnitude").and_then(|m| m.as_f64()).unwrap_or(0.0);
                        let (modifier, detrimental) = self.effect(b);
                        if let (true, Some(code)) = (modifier, code) {
                            out.changes.push((code, magnitude, detrimental));
                        }
                    }
                    _ => {}
                }
            }
        }
        self.enchs.insert(id, out.clone());
        out
    }

    /// The hints' items: the DOBJ list's Stimpak, Doctor's Bag, RadAway and RadX
    /// ([`HINT_PLACES`]), each `None` when the list names none there.
    pub fn hint_items(&mut self) -> [Option<u32>; 4] {
        let lo = self.lo;
        *self.hint_items.get_or_insert_with(|| default_objects(lo, HINT_PLACES))
    }

    /// An item's enchantment (EITM) [observed: ARMO records carry it, e.g. the Vault 21
    /// Jumpsuit's `EnchClothingVaultSuitNormal`].
    pub fn item_ench(&mut self, item: u32) -> Option<u32> {
        let (lo, schema) = (self.lo, self.schema);
        *self.item_ench.entry(item).or_insert_with(|| fields(lo, schema, item).and_then(|(_, f)| f.formid(b"EITM", lo)))
    }
}

/// The items the status page's hints count, by Editor ID [observed: FalloutNV.esm ALCH
/// `Stimpak` 0x15169, `NVDoctorBag` 0xCB05C, `RadAway` 0x15167, `RadX` 0x15168]: the
/// fallback when the DOBJ list names none ([`HINT_PLACES`]).
const HINT_ITEMS: [&str; 4] = ["Stimpak", "NVDoctorBag", "RadAway", "RadX"];

/// The DOBJ places of the hints' items, in the hints' order: 0 Stimpak, 21 Doctor's Bag,
/// 3 RadAway, 2 RadX [documented: xEdit wbDefinitionsFNV.pas DOBJ names, in order;
/// docs/spec-notes/default-objects.md §1; observed: FalloutNV.esm names the four items of
/// [`HINT_ITEMS`] there].
const HINT_PLACES: [usize; 4] = [0, 21, 3, 2];

/// The default objects (DOBJ `DATA`, the winning definition of the last plugin that has
/// one) at `places`, as global FormIDs; `None` where the list names nothing.
fn default_objects(lo: &LoadOrder, places: [usize; 4]) -> [Option<u32>; 4] {
    let mut list: Vec<u32> = Vec::new();
    for (pi, records) in lo.records.iter().enumerate() {
        for r in records.iter().filter(|r| &r.header.sig == b"DOBJ" && r.header.flags & fnv_world::placed::ref_flags::DELETED == 0) {
            let Ok(subs) = r.subrecords() else { continue };
            if let Some(d) = subs.iter().find(|s| &s.sig == b"DATA") {
                list = d.data.as_chunks::<4>().0.iter().map(|c| global(lo, pi, u32::from_le_bytes(*c)).filter(|&x| x != 0).unwrap_or(0)).collect();
            }
        }
    }
    places.map(|i| list.get(i).copied().filter(|&x| x != 0))
}

/// The player's base form (NPC_ 0x7) [documented: GECK wiki, the player is 0x14 placed
/// of base 0x7].
const PLAYER_BASE: FormId = FormId(7);

/// Read everything the page shows. Without the scripts' game the figures it keeps are
/// left out ("--").
pub fn snapshot(forms: &mut Forms, sim: &mut SimState, script: Option<&mut ScriptState>) -> Snapshot {
    let mut s = Snapshot { name: sim.name(PLAYER_BASE).unwrap_or("").to_string(), limbs: [1.0; 6], ..Snapshot::default() };
    // The hints' counts.
    let mut texts: [String; 4] = Default::default();
    let mut counts = [0; 4];
    let dobj = forms.hint_items();
    for (i, edid) in HINT_ITEMS.iter().enumerate() {
        let id = dobj[i].map(FormId).or_else(|| sim.forms.by_edid(edid));
        counts[i] = id.and_then(|id| sim.world.item_count(FormId::PLAYER_REF, id).ok()).unwrap_or(0);
        let name = id.and_then(|id| sim.name(id)).unwrap_or(edid).to_string();
        texts[i] = model::hint_text(&name, counts[i]);
    }
    s.hints = Hints::new(texts, counts);
    // [design] As the engine's other menu words: the running game's strings, then
    // the repository's own string-table label (menu::look::quantity_words).
    s.switch_limbs = match script.as_ref().and_then(|s| s.rt.as_ref()) {
        Some(rt) => fnv_game::ui_loading::game_string(&rt.game, "sSwitchLimbs"),
        None => fnv_ui::strings::built_in("sSwitchLimbs").unwrap_or_default().to_owned(),
    };
    let xp_base = sim.forms.setting_f32("iXPBase").unwrap_or(200.0) as f64;
    let xp_bump = sim.forms.setting_f32("iXPBumpBase").unwrap_or(150.0) as f64;
    // The karma titles the plugins hold (`model::karma_title`).
    let karma_titles: std::collections::HashMap<String, String> = sim
        .forms
        .settings
        .iter()
        .filter_map(|(k, v)| match v {
            fnv_sim::settings::Setting::Str(t) if k.starts_with("skarmictitle") => Some((k.clone(), t.clone())),
            _ => None,
        })
        .collect();
    let karma_limits = ["KarmaVeryEvil", "KarmaEvil", "KarmaGood", "KarmaVeryGood"].map(|g| sim.forms.global_by_edid(g).map(|v| v.1 as f64));
    // The lists' words do not need the game.
    s.special = SPECIALS.map(|c| av_row(forms, c)).collect();
    // A weapon skill has its mark, as a weapon has on the ITEMS page (`items::skill_badge`)
    // [observed: private job run07b_stats, these five skills' and no others'].
    let mut skills: Vec<Row> = SKILLS
        .filter(|&c| c != av::BIG_GUNS)
        .map(|c| {
            let row = av_row(forms, c);
            Row { badge: crate::pipboy::items::skill_badge(&row.name).map(str::to_string), ..row }
        })
        .collect();
    s.general = model::MISC_STATS.iter().enumerate().map(|(i, n)| Row { key: RowKey::Stat(i as u8), name: n.to_string(), value: Some("--".into()), ..Row::default() }).collect();
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else {
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        s.skills = skills;
        return s;
    };
    s.game = true;
    crate::script::with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        let p = FormId::PLAYER_REF;
        let value = shown_and_permanent;
        let level = g.player_level();
        s.level = Some(level as i64);
        // The statistics' counters (`fnv_game::misc_stats`), by name.
        for row in &mut s.general {
            if let Some(code) = fnv_game::misc_stats::code(&row.name) {
                row.value = Some(format!("{}", g.misc_stat(code).round() as i64));
            }
        }
        // The player's tweak sorts them by name [design: docs/design/tweaks.md].
        if g.tweaks.sort_misc_stats {
            s.general.sort_by(|a, b| a.name.cmp(&b.name));
        }
        // HP and AP out of a whole that an aid item's temporary modifier raises with them
        // (Buffout, Jet) [hypothesis].
        let whole = |g: &mut fnv_game::Game<'static>, code: u16| {
            let t = gf::actor_value_temporary(g, FormId::PLAYER_REF, code);
            value(g, code).map(|(v, max)| (v, max + t))
        };
        s.hp = whole(g, av::HEALTH);
        s.ap = whole(g, av::ACTION_POINTS);
        s.xp = Some((g.player.xp, model::xp_for_level(level + 1.0, xp_base, xp_bump)));
        // The player's tweak: the way through this level as a percentage [design: docs/design/tweaks.md].
        if g.tweaks.stats_xp_percent {
            let (from, to) = (model::xp_for_level(level, xp_base, xp_bump), model::xp_for_level(level + 1.0, xp_base, xp_bump));
            s.xp_percent = Some(if to > from { ((g.player.xp - from) / (to - from) * 100.0).clamp(0.0, 100.0) } else { 100.0 });
        }
        for (i, &code) in LIMBS.iter().enumerate() {
            if let Some((v, max)) = value(g, code).filter(|v| v.1 > 0.0) {
                s.limbs[i] = (v / max).clamp(0.0, 1.0);
            }
        }
        // The CND hints lit while they would heal (`model::refusal` on the body).
        let hardcore = g.hardcore();
        for hint in [model::STIMPAK, model::DOCTORS_BAG] {
            let hp = s.hp.unwrap_or((0.0, 0.0));
            s.hints.lit[hint] = model::refusal_with(hint, None, s.hints.counts[hint], hp, &s.limbs, hardcore, g.tweaks.aid_at_full_health).is_none();
        }
        s.rad_resist = value(g, av::RAD_RESISTANCE).map(|v| v.0);
        s.rads = value(g, 54).map_or(0.0, |v| v.0.max(0.0));
        s.hardcore = hardcore;
        s.needs = [fnv_game::aid::DEHYDRATION, fnv_game::aid::HUNGER, fnv_game::aid::SLEEP_DEPRIVATION].map(|code| value(g, code).map_or(0.0, |v| v.0.max(0.0)));
        for (rows, codes) in [(&mut s.special, SPECIALS.collect::<Vec<_>>()), (&mut skills, SKILLS.filter(|&c| c != av::BIG_GUNS).collect())] {
            for (row, code) in rows.iter_mut().zip(codes) {
                if let Some((shown, permanent)) = value(g, code) {
                    // The player's tweak: past the cap, and the amount in the mark [design: docs/design/tweaks.md].
                    let detailed = g.tweaks.detailed_stats;
                    let cap = |v: f64| if detailed { v } else { av::capped(code, v) };
                    let (shown, permanent) = (cap(shown), cap(permanent));
                    row.value = Some(model::whole(shown));
                    row.mark = model::modified_mark(shown, permanent).map(|m| if detailed { format!("({:+})", (shown.round() - permanent.round()) as i64) } else { m.to_string() });
                }
            }
        }
        // The player's tweak: the tag skills in bold [design: docs/design/tweaks.md].
        if g.tweaks.embolden_tags {
            for row in &mut skills {
                row.bold = matches!(row.key, RowKey::Av(code) if g.player.tag_skills.contains(&code));
            }
        }
        // Perks the player has that are not Hidden, Playable or not (`PerkData::listed`)
        // [hypothesis: traits are listed with them], by name; the rank when the perk has
        // more than one.
        let mut perks: Vec<Row> = Vec::new();
        for (&perk, &rank) in &g.player.perks {
            let d = forms.perk(perk.0);
            if !d.listed() {
                continue;
            }
            let mut t = forms.text(perk.0).clone();
            // A perk no plugin has (NVForever's co-op perks, `fnv_game::coop_perks`): its
            // words and picture from the game's data.
            if t.name.is_empty() {
                if let Some(p) = g.data.perks.get(&perk) {
                    t = Text { name: p.name.clone(), description: p.description.clone(), icon: p.icon.clone(), short: None };
                }
            }
            perks.push(Row { key: RowKey::Perk(perk), name: t.name, value: (d.ranks > 1).then(|| rank.to_string()), mark: None, description: t.description, icon: t.icon, badge: None, bold: false });
        }
        perks.sort_by(|a, b| a.name.cmp(&b.name));
        s.perks = perks;
        // Reputations with any fame or infamy [hypothesis: the list shows those].
        let mut standings: Vec<Standing> = Vec::new();
        for (&rep, &[infamy, fame]) in &g.player.reputation {
            if fame <= 0.0 && infamy <= 0.0 {
                continue;
            }
            let scale = g.data.reputations.get(&rep).copied().unwrap_or(0.0) as f64;
            let t = forms.text(rep.0).clone();
            let title = model::reputation_title(gf::reputation_level(fame, scale), gf::reputation_level(infamy, scale));
            // The player's tweak shows fame and infamy [design: docs/design/tweaks.md].
            let name = if g.tweaks.show_fame_infamy { format!("{} (fame {fame:.0}, infamy {infamy:.0})", t.name) } else { t.name };
            standings.push(Standing { form: rep, name, title, fame, infamy, icon: t.icon });
        }
        standings.sort_by(|a, b| a.name.cmp(&b.name));
        s.hide_reputation = g.tweaks.hide_empty_reputation && standings.is_empty();
        s.standings = standings;
        if let (Some((karma, _)), [Some(ve), Some(e), Some(go), Some(vg)]) = (value(g, av::KARMA), karma_limits) {
            let (word, picture) = model::karma_alignment(karma, ve, e, go, vg);
            s.karma = Some((karma, word, picture));
            let setting = |n: &str| karma_titles.get(&n.to_ascii_lowercase()).cloned();
            let ttw = g.ttw_karma_titles();
            s.karma_title = Some(model::karma_title_with_ttw(level as i64, word, ttw, setting));
            s.karma_setting_title = model::karma_title_setting_with_ttw(level as i64, word, ttw, setting);
        }
        // Worn apparel's enchantments taking effect now, as the game holds them: one
        // line per worn item, its Value Modifier effects (`Forms::effect`) at the amounts
        // the game applied [hypothesis: the line is named by the enchantment, else the
        // item]. Inactive and dispelled effects are left out (#2495).
        for ((item, ench), effects) in worn_effects(g) {
            let changes: Vec<Change> = effects
                .iter()
                .filter_map(|x| {
                    let c = x.change?;
                    let (modifier, detrimental) = forms.effect(x.effect.0);
                    modifier.then(|| Change { value: short(forms, c.code), amount: c.amount.abs(), detrimental })
                })
                .collect();
            if changes.is_empty() {
                continue;
            }
            let mut name = forms.text(ench.0).name.clone();
            if name.is_empty() {
                name = forms.text(item.0).name.clone();
            }
            s.effects.push(EffectLine { name, text: model::changes_text(&changes) });
        }
        // Aid items taking effect now (ALCH sources of active effects), after the worn
        // apparel [hypothesis: the order]. An effect whose conditions fail (a Stimpak's
        // Hardcore heal outside Hardcore) is held inactive and changes nothing: not
        // listed (#1438).
        let mut seen: Vec<FormId> = Vec::new();
        for e in g.effects.iter().filter(|e| e.target == p && !e.inactive) {
            if seen.contains(&e.source) || g.data.sig(e.source) != Some(*b"ALCH") {
                continue;
            }
            seen.push(e.source);
            let changes: Vec<Change> = g
                .effects
                .iter()
                .filter(|x| x.target == p && x.source == e.source && !x.inactive)
                .filter_map(|x| {
                    let (modifier, detrimental) = forms.effect(x.effect.0);
                    let code = forms.ench_code(e.source.0, x.index)?;
                    // The amount the game applied (potency included, `fnv_game::aid`).
                    let amount = x.change.map_or(x.magnitude as f64, |c| c.amount.abs());
                    modifier.then(|| Change { value: short(forms, code), amount, detrimental })
                })
                .collect();
            // Script effects shown by name (Psycho's "Damage +25%"), after the values (#3119).
            let mut named: Vec<FormId> = Vec::new();
            for x in g.effects.iter().filter(|x| x.target == p && x.source == e.source && !x.inactive) {
                if !named.contains(&x.effect) && g.data.magic_effects.get(&x.effect).is_some_and(|m| m.shows_name()) {
                    named.push(x.effect);
                }
            }
            let text = with_names(model::changes_text(&changes), named.iter().map(|m| forms.text(m.0).name.clone()));
            if !text.is_empty() {
                // The player's tweak adds the seconds left [design: docs/design/tweaks.md].
                let name = forms.text(e.source.0).name.clone();
                let name = if g.tweaks.effect_time_left && e.duration > 0.0 { format!("{name} ({:.0}s)", (e.duration - e.elapsed).max(0.0)) } else { name };
                s.effects.push(EffectLine { name, text });
            }
        }
    });
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    s.skills = skills;
    s
}

/// An item's effect text: its value changes, then the names of its effects shown by name
/// ([`fnv_game::data::MagicEffectDef::shows_name`]), comma-separated [hypothesis: the
/// original's layout is not captured, #3119]; empty names are left out.
pub fn with_names(values: String, names: impl IntoIterator<Item = String>) -> String {
    let mut parts: Vec<String> = if values.is_empty() { Vec::new() } else { vec![values] };
    parts.extend(names.into_iter().filter(|n| !n.trim().is_empty()));
    parts.join(", ")
}

/// The player's health (now, most), limb conditions, Hardcore, and the Aid at Full
/// Health tweak.
pub type Condition = ((f64, f64), [f64; 6], bool, bool);

/// The player's health (current and whole, as the heading), limb conditions (0 to 1, as
/// the CND bars) and whether Hardcore mode is on: what the CND page's hints are judged
/// by; `None` without the scripts' game.
pub fn condition(sim: &mut SimState, script: Option<&mut ScriptState>) -> Option<Condition> {
    let rt = script?.rt.as_mut()?;
    Some(crate::script::with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        let t = gf::actor_value_temporary(g, FormId::PLAYER_REF, av::HEALTH);
        let hp = shown_and_permanent(g, av::HEALTH).map_or((0.0, 0.0), |(v, max)| (v, max + t));
        let mut limbs = [1.0; 6];
        for (i, &code) in LIMBS.iter().enumerate() {
            if let Some((v, max)) = shown_and_permanent(g, code).filter(|v| v.1 > 0.0) {
                limbs[i] = (v / max).clamp(0.0, 1.0);
            }
        }
        (hp, limbs, g.hardcore(), g.tweaks.aid_at_full_health)
    }))
}

/// The hint's item (Stimpak, Doctor's Bag, RadAway, Rad-X), by its Editor ID.
pub fn hint_item(sim: &SimState, hint: usize) -> Option<FormId> {
    HINT_ITEMS.get(hint).and_then(|e| sim.forms.by_edid(e))
}

/// An actor value of the player: what the page shows (base, permanent and temporary
/// modifiers, less damage) and its permanent value (base and permanent modifier). The
/// temporary modifier already holds the worn apparel's enchantments, active effects
/// since #377 (`fnv_game::abilities` `sync_worn`) [documented: GECK wiki ActorValue,
/// armour's object effects "all of them target the temporary pool"], and the aid
/// items'; the page adds nothing of its own (#2495).
fn shown_and_permanent(g: &mut fnv_game::Game<'_>, code: u16) -> Option<(f64, f64)> {
    let [b, m, d] = gf::actor_value_parts(g, FormId::PLAYER_REF, code).ok()?;
    let t = gf::actor_value_temporary(g, FormId::PLAYER_REF, code);
    Some((b + m + t - d, b + m))
}

/// The worn apparel's enchantments taking effect on the player now, as the game holds
/// them: each (worn item, enchantment) with its active effects, in the game's order. An
/// effect whose conditions fail is held inactive and changes nothing, and a dispelled one
/// is gone: neither is here (#2495).
fn worn_effects<'g>(g: &'g fnv_game::Game<'_>) -> Vec<((FormId, FormId), Vec<&'g ActiveEffect>)> {
    let mut out: Vec<((FormId, FormId), Vec<&ActiveEffect>)> = Vec::new();
    for e in g.effects.iter().filter(|e| e.target == FormId::PLAYER_REF && !e.inactive) {
        let Some(item) = e.worn else { continue };
        let key = (item, e.source);
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(e),
            None => out.push((key, vec![e])),
        }
    }
    out
}

/// The player's health out of its whole, as the STATS page's HP card reads it
/// ([`snapshot`]); `None` without the scripts' game. For the ITEMS page's HP card.
pub fn player_hp(sim: &mut SimState, script: Option<&mut ScriptState>) -> Option<(f64, f64)> {
    let rt = script.and_then(|s| s.rt.as_mut())?;
    crate::script::with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        let t = gf::actor_value_temporary(g, FormId::PLAYER_REF, av::HEALTH);
        shown_and_permanent(g, av::HEALTH).map(|(v, max)| (v, max + t))
    })
}

/// An actor value's short name, else its name.
fn short(forms: &mut Forms, code: u16) -> String {
    let t = forms.value(code);
    t.short.unwrap_or(t.name)
}

/// A list row of an actor value, its figure not read yet.
fn av_row(forms: &mut Forms, code: u16) -> Row {
    let t = forms.value(code);
    Row { key: RowKey::Av(code), name: t.name, value: Some("--".into()), mark: None, description: t.description, icon: t.icon, badge: None, bold: false }
}

impl Forms {
    /// An actor value's short name (AVIF ANAM), else its name.
    pub fn short_name(&mut self, code: u16) -> String {
        short(self, code)
    }

    /// The actor value of effect `index` of a magic item (its `index`th EFIT).
    fn ench_code(&mut self, item: u32, index: usize) -> Option<u16> {
        let (_, f) = fields(self.lo, self.schema, item)?;
        let s = f.subs.iter().filter(|s| &s.sig == b"EFIT").nth(index)?;
        f.decode(s)?.get("actor_value")?.as_str().and_then(gf::av_by_name)
    }
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
