//! Lutana NVSE (LU, merged into JIP LN), batch 3 (#1009; docs/spec-notes/ext-lutana.md
//! §10): the last 24. Appearance and base swaps, equipping a unit of given health and
//! mods, perk ranks and perk entries, the activator prompt, the Anim Object idle, the
//! loading screen type's text colour and the loading screen's texture, the player's
//! regions, the cell enter and exit events, and three deprecated ones (GetCellRefs,
//! SetVariableFloat, BaseAddItemHealth) [documented: each page in the wiki export].
//! Record setters change the record for the session (`Game::field_overrides`), read back
//! by the getters; a perk's entries are copied for the session and the perk effects read
//! the copy (`Game::perk_def`).

use crate::data::{PerkDef, PerkEffect};
use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::HashMap;

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

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// The session state of batch 3.
#[derive(Debug, Clone, Default)]
pub struct Lu3State {
    /// Perks whose definition a script changed for the session (entries, `SetPerkLevel`,
    /// `SetPerkFlag`, `LNSetIsPlayable`): a copy of the record with the changes. Carried
    /// across same-run loads via `Game::take_session` / `put_session`, not saved
    /// [hypothesis: session-only, like other record setters].
    pub perk_edits: HashMap<FormId, PerkDef>,
    /// CopyFaceGenFrom's backup of an actor's look before its first copy, to undo it:
    /// its race, its sex and how many face matches it had.
    pub face_backup: HashMap<FormId, (Option<FormId>, bool, usize)>,
    /// The player's cell at the last frame, for OnCellEnter and OnCellExit.
    pub last_cell: Option<FormId>,
}

impl Game<'_> {
    /// A perk's definition as the session has it: the copy a script changed, else the
    /// record's.
    pub fn perk_def(&self, perk: FormId) -> Option<&PerkDef> {
        self.lu3.perk_edits.get(&perk).or_else(|| self.data.perks.get(&perk))
    }

    /// Mutable session copy of a perk's definition (cloned from the authored record on
    /// first edit); `None` when `perk` is not a `PERK` form.
    pub(crate) fn perk_def_mut(&mut self, perk: FormId) -> Option<&mut PerkDef> {
        if !self.lu3.perk_edits.contains_key(&perk) {
            let def = self.data.perks.get(&perk)?.clone();
            self.lu3.perk_edits.insert(perk, def);
        }
        self.lu3.perk_edits.get_mut(&perk)
    }

    /// Each frame (`Game::tick`): the player's cell changed, so OnCellExit with the cell
    /// left, then OnCellEnter with the cell entered [documented: the pages, "when the
    /// player changes cells"; hypothesis: the exit first]. Exterior cells count when the
    /// player crosses their borders [documented: OnCellEnter's note]. Nothing the first
    /// frame (no cell left yet) [hypothesis].
    pub(crate) fn lu3_tick(&mut self) {
        let now = self.cell_of(FormId::PLAYER_REF);
        let was = std::mem::replace(&mut self.lu3.last_cell, now);
        if now == was || was.is_none() {
            return;
        }
        if let Some(c) = was {
            self.dispatch_event("OnCellExit", vec![XVal::Ref(c.0)], None);
        }
        if let Some(c) = now {
            self.dispatch_event("OnCellEnter", vec![XVal::Ref(c.0)], None);
        }
    }
}

/// The base of a reference, or the form itself.
fn base(g: &Game, f: FormId) -> FormId {
    crate::functions::base_of(g, f).unwrap_or(f)
}

fn of_sig(g: &Game, f: FormId, sigs: &[&[u8; 4]]) -> bool {
    g.data.sig(f).is_some_and(|s| sigs.iter().any(|x| **x == s))
}

/// Where the script functions' Nth entry of a perk is in the record: the entries by rank,
/// then priority, then kind (quest stages, abilities, entry points), those alike in the
/// reverse of the record's order [observed: lu3 perkentry, Fast Shot's entry 1 is its
/// third in the record (entry points 40, 43, 8, 34 in the record, 8 read); gaps4 and
/// jip2, Toughness's entry 0 is its rank-1 entry, the first of two ranks; L5.ext rerun,
/// #2040: Small Frame (ability, ability, entry point 6 in the record) reads abilities
/// at 0 and 1, Built to Last (ability, entry point) its ability at 0; hypothesis: the
/// kind as the key, the one rule found that fits every perk probed, and the order
/// between priorities, no perk probed has two].
pub(crate) fn nth_entry(def: &PerkDef, i: f64) -> Option<usize> {
    let i = usize::try_from(i as i64).ok()?;
    let kind = |e: &crate::data::PerkEntry| match e.effect {
        PerkEffect::QuestStage { .. } => 0u8,
        PerkEffect::Ability(_) => 1,
        PerkEffect::EntryPoint(_) => 2,
        PerkEffect::Other => 3,
    };
    let mut order: Vec<usize> = (0..def.entries.len()).collect();
    order.sort_by_key(|&k| { let e = &def.entries[k]; (e.rank, e.priority, kind(e), std::cmp::Reverse(k)) });
    order.get(i).copied()
}

/// The perk's Nth entry ([`nth_entry`]) as the session has it.
pub(crate) fn nth_entry_of<'g>(g: &'g Game, perk: FormId, i: f64) -> Option<&'g crate::data::PerkEntry> {
    let d = g.perk_def(perk)?;
    d.entries.get(nth_entry(d, i)?)
}

/// The perk's Nth entry to change for the session (a copy made the first time).
pub(crate) fn perk_entry_mut<'g>(g: &'g mut Game, perk: FormId, i: f64) -> Option<&'g mut crate::data::PerkEntry> {
    let d = g.perk_def_mut(perk)?;
    let k = nth_entry(d, i)?;
    d.entries.get_mut(k)
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- the activator prompt (ACTI XATO) [documented: Get/SetActivatorPrompt,
        // `[activator]` or the calling reference's base]; "" without one [hypothesis].
        "getactivatorprompt" => {
            let Some(a) = form(args, 0).or(subject).map(|f| base(g, f)).filter(|&a| of_sig(g, a, &[b"ACTI"])) else { return Some(Ok(XVal::Str(String::new()))) };
            match crate::nvse::record_value(g, a, "XATO", "") {
                Some(XVal::Str(s)) => XVal::Str(s),
                _ => XVal::Str(String::new()),
            }
        }
        "setactivatorprompt" => {
            if let Some(a) = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&a| of_sig(g, a, &[b"ACTI"])) {
                g.field_overrides.insert((a, "XATO/".into()), XVal::Str(text(args, 0)));
            }
            zero
        }
        // ---- the Anim Object's idle (ANIO DATA) [documented: Get/SetAnimObjectIdle];
        // the setter takes an IDLE only (see docs/spec-notes/ext-lutana.md §10).
        "getanimobjectidle" | "getanioidle" => {
            let Some(a) = form(args, 0).filter(|&a| of_sig(g, a, &[b"ANIO"])) else { return Some(Ok(XVal::Ref(0))) };
            match crate::nvse::record_value(g, a, "DATA", "") {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            }
        }
        "setanimobjectidle" | "setanioidle" => {
            if let (Some(a), Some(i)) = (form(args, 0).filter(|&a| of_sig(g, a, &[b"ANIO"])), form(args, 1).filter(|&i| of_sig(g, i, &[b"IDLE"]))) {
                g.field_overrides.insert((a, "DATA/".into()), XVal::Ref(i.0));
            }
            zero
        }
        // ---- the loading screen type's text colour: the first font's [documented: the
        // page's example, StandardLoadingTip r 255 g 180 b 53; observed: those are LSCT
        // DATA font 1's, docs/spec-notes/loading-screens.md]. A stringmap r, g, b.
        "getloadscreentypetextrgb" => {
            let Some(t) = form(args, 0).filter(|&t| of_sig(g, t, &[b"LSCT"])) else { return Some(Ok(XVal::Array(0))) };
            let entries = [("r", "font_1_red"), ("g", "font_1_green"), ("b", "font_1_blue")]
                .iter()
                .map(|(k, p)| (XKey::Str((*k).into()), XVal::Num(crate::nvse::record_value(g, t, "DATA", p).and_then(|v| v.num().ok()).unwrap_or(0.0))))
                .collect();
            XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
        }
        // Raw floats stored verbatim as f32 (no 0..255 clamping, and NaN stored verbatim)
        // [documented: the schema's LSCT; observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23/24, GSDocMitchellHouse (0x00103DF9),
        // 10 official base masters, `jip_09_lsct_text_rgb_clamp`
        // (`tools/capture/nvse/catalogs/wave4a_extenders.json`):
        // `PROBE|jip_09_lsct_text_rgb_clamp|n0=3,init=(255.0,180.0,53.0),over=(59.3,181.0,300.0),neg=(-10.0,100.0,50.0)`
        // and `w4d_25_lsct_rgb_nan_and_password_key`
        // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679 / #3803):
        // `PROBE|w4d_25_lsct_rgb_nan_and_password_key|lsct(0=255.0,180.0,53.0|nan=-nan(ind),123.5,45.0)|door(0->1)|termRef(0->1)`;
        // no DLL or source consulted].
        "setloadscreentypetextrgb" => {
            if let Some(t) = form(args, 0).filter(|&t| of_sig(g, t, &[b"LSCT"])) {
                for (k, p) in ["font_1_red", "font_1_green", "font_1_blue"].iter().enumerate() {
                    if let Some(XVal::Num(v)) = args.get(k + 1) {
                        g.field_overrides.insert((t, format!("DATA/{p}")), XVal::Num(f64::from(*v as f32)));
                    }
                }
            }
            zero
        }
        // The loading screen's texture (LSCR ICON) [documented: SetLoadScreenTexture,
        // under textures]. The loading screens drawn read the record [not built: engine
        // follow-up].
        "setloadscreentexture" => {
            if let Some(s) = form(args, 0).filter(|&s| of_sig(g, s, &[b"LSCR"])) {
                g.field_overrides.insert((s, "ICON/".into()), XVal::Str(text(args, 1)));
            }
            zero
        }
        // ---- actor bases
        // `npc race`: the NPC's race (RNAM); not the player's [documented: SetRace, "doesn't
        // work on the Player"]. The look changes when the 3D is built again [documented:
        // "Update3D is required"; the drawn actor reads the record: engine follow-up].
        "setrace" => {
            if let (Some(n), Some(r)) = (form(args, 0).map(|f| base(g, f)), form(args, 1).filter(|&r| of_sig(g, r, &[b"RACE"]))) {
                if of_sig(g, n, &[b"NPC_"]) && n != FormId::PLAYER_BASE {
                    g.field_overrides.insert((n, "RNAM/".into()), XVal::Ref(r.0));
                }
            }
            zero
        }
        // `template [actorBase]`: TPLT of the base, or the calling reference's [documented:
        // LNSetActorTemplate]; an NPC or creature, a template of either or a leveled list
        // [documented: xEdit TPLT; hypothesis: others refused]. GetActorTemplate reads it;
        // the template resolution of the actor's stats reads the record [not built].
        "lnsetactortemplate" => {
            let a = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&a| of_sig(g, a, &[b"NPC_", b"CREA"]));
            let t = form(args, 0).filter(|&t| of_sig(g, t, &[b"NPC_", b"CREA", b"LVLN", b"LVLC"]));
            if let (Some(a), Some(t)) = (a, t) {
                g.field_overrides.insert((a, "TPLT/".into()), XVal::Ref(t.0));
            }
            zero
        }
        // `flag state [actorBase]`: a Template Data flag, 1 Use Traits … 512 Use Script
        // [documented: the page's table; ACBS template_flags]. One flag of the table at a
        // time [hypothesis: the IDs are single bits].
        "setactortemplateflag" => {
            let flag = num(args, 0, 0.0) as u64;
            if let Some(a) = form(args, 2).or(subject).map(|f| base(g, f)).filter(|&a| of_sig(g, a, &[b"NPC_", b"CREA"])) {
                if flag.is_power_of_two() && flag <= 512 {
                    let old = crate::nvse::record_value(g, a, "ACBS", "template_flags").and_then(|v| v.num().ok()).unwrap_or(0.0) as u64;
                    let new = if num(args, 1, 0.0) != 0.0 { old | flag } else { old & !flag };
                    g.field_overrides.insert((a, "ACBS/template_flags".into()), XVal::Num(new as f64));
                }
            }
            zero
        }
        // `[sourceNPC]` on an actor: the source's race, sex and face geometry; none undoes
        // it [documented: CopyFaceGenFrom, "keeps an internal backup", "call the function
        // omitting the sourceNPC argument"]. Through the appearance changes AgeRace,
        // SexChange and MatchFaceGeometry use (`crate::appearance`), which the engine
        // draws again at once [documented: "Visual update is already performed"]: on the
        // calling actor, not every reference of its base [decision: NVForever keeps a
        // look per reference, saved with it; the page changes the base]. The player's
        // face geometry is not copied (the character creator's look is matched only to a
        // face the game holds); hair, eyes and skin tone stay [not built: the drawn head
        // reads them from the record, an engine follow-up].
        "copyfacegenfrom" => {
            let Some(r) = subject.filter(|&r| of_sig(g, base(g, r), &[b"NPC_"])) else { return Some(Ok(zero)) };
            match form(args, 0).map(|f| base(g, f)) {
                Some(src) if of_sig(g, src, &[b"NPC_"]) => {
                    if !g.lu3.face_backup.contains_key(&r) {
                        let kept = g.extra.get(&r).map_or(0, |e| e.face_matches.len());
                        let backup = (g.actor_race(r), g.actor_female(r), kept);
                        g.lu3.face_backup.insert(r, backup);
                    }
                    // The source NPC record's own race and sex [hypothesis: not through its
                    // templates; the page names the NPC form].
                    let (race, female) = g.data.actors.get(&src).map_or((None, false), |d| (d.race, d.female));
                    if let Some(race) = race {
                        g.change_race(r, race);
                    }
                    g.sex_change(r, Some(female), false);
                    if r != FormId::PLAYER_REF {
                        let _ = g.match_face_geometry(r, src, 100.0);
                    }
                }
                Some(_) => {}
                None => {
                    if let Some((race, female, kept)) = g.lu3.face_backup.remove(&r) {
                        if let Some(race) = race {
                            g.change_race(r, race);
                        }
                        g.sex_change(r, Some(female), false);
                        if let Some(e) = g.extra.get_mut(&r) {
                            e.face_matches.truncate(kept);
                        }
                        g.appearance_changes.push(crate::appearance::AppearanceChange { actor: r, reset: false });
                    }
                }
            }
            zero
        }
        // `newBase` on a reference: its base for the session, of another type too
        // [documented: SetBaseForm, JIP LN 56.70]. The drawn reference changes when its
        // 3D is built again (see docs/spec-notes/ext-lutana.md §10; engine follow-up).
        "setbaseform" => {
            if let (Some(r), Some(b)) = (subject, form(args, 0)) {
                if g.data.sig(b).is_some() && r != FormId::PLAYER_REF {
                    if let Ok(s) = g.world.get(r) {
                        s.base = b;
                    }
                }
            }
            zero
        }
        // ---- equipping
        // `item [health] [modFlags] [noUnequip] [noMessage]`: the unit of that health (-1
        // or none any) and mods (none any) [documented: EquipItemData]; the health is the
        // item's health points, as GetEquippedData gives [hypothesis: the page names no
        // unit; GetEquippedData's health is the points]. OnEquip runs [documented].
        "equipitemdata" => {
            let (Some(a), Some(item)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let health = num(args, 1, -1.0);
            let mods = args.get(2).map(|_| num(args, 2, 0.0) as u8);
            equip(g, a, item, health, mods, num(args, 3, 0.0) != 0.0);
            zero
        }
        // `equippedData [noUnequip] [noMessage]`: GetEquippedData's [item, health, mods]
        // [documented: SetEquippedData].
        "setequippeddata" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let id = match args.first() {
                Some(XVal::Array(id)) => *id,
                Some(XVal::Num(x)) => *x as u32,
                _ => return Some(Ok(zero)),
            };
            let vals: Vec<XVal> = g.ext.array(id).map(|arr| arr.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
            if let Some(item) = form(&vals, 0) {
                let mods = Some(num(&vals, 2, 0.0) as u8);
                equip(g, a, item, num(&vals, 1, -1.0), mods, num(args, 1, 0.0) != 0.0);
            }
            zero
        }
        // ---- perks
        // `perk rank [forTeammates]`: the player's rank set (added at it when missing), or
        // the teammates' perk; another actor's held rank [documented: SetPerkRank; the
        // NPC side as AddPerk on an NPC, `RefExtra::perks`]. A rank above the perk's ranks
        // is cut to them; 0 changes nothing [observed: lu3 perkrank, Educated set to 9
        // reads 1, then set to 0 still 1].
        "setperkrank" => {
            let (a, Some(p)) = (subject.unwrap_or(FormId::PLAYER_REF), form(args, 0).filter(|&p| g.data.perks.contains_key(&p))) else { return Some(Ok(zero)) };
            let max = g.data.perks.get(&p).map_or(1, |d| d.ranks.max(1));
            let rank = (num(args, 1, 0.0).max(0.0) as u8).min(max);
            // rank == 0 (or negative, clamped to 0) leaves the held rank unchanged
            // [observed: lu3 perkrank; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
            // JIP LN 57.30, 2026-11-23, GSDocMitchellHouse (0x00103DF9), 10 official
            // base masters, `b3_lut_13_set_perk_rank_clamp`
            // (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679):
            // `PROBE|b3_lut_13_set_perk_rank_clamp|r0=0,after9=1,after0=1`; no DLL or
            // source consulted].
            if rank == 0 {
                return Some(Ok(zero));
            }
            if a != FormId::PLAYER_REF {
                let held = &mut g.extra.entry(a).or_default().perks;
                held.insert(p, rank);
                g.sync_perk_abilities_of(a);
            } else if num(args, 2, 0.0) != 0.0 {
                g.player.teammate_perks.insert(p);
            } else {
                let before = g.player.perks.get(&p).copied().unwrap_or(0);
                if rank > before {
                    while g.player.perks.get(&p).copied().unwrap_or(0) < rank {
                        crate::perks::gain_rank(g, p);
                    }
                } else if rank < before {
                    g.player.perks.insert(p, rank);
                }
                let after = g.player.perks.get(&p).copied().unwrap_or(0);
                if after != before {
                    g.perk_event(p, after > before, before, after);
                }
            }
            zero
        }
        // `perk index form`: an Ability entry's spell, a Quest + Stage entry's quest
        // [documented: SetNthPerkEntryForm, "Ability for type 5, Quest for type 6"].
        "setnthperkentryform" => {
            if let (Some(p), Some(f)) = (form(args, 0), form(args, 2)) {
                let is_spell = of_sig(g, f, &[b"SPEL"]);
                let is_quest = of_sig(g, f, &[b"QUST"]);
                if let Some(e) = perk_entry_mut(g, p, num(args, 1, -1.0)) {
                    match &mut e.effect {
                        PerkEffect::Ability(s) if is_spell => *s = f,
                        PerkEffect::QuestStage { quest, .. } if is_quest => *quest = f,
                        _ => {}
                    }
                }
            }
            zero
        }
        // `perk index code`: an entry point entry's entry point, 0-73 [documented: the
        // page's table]; the perk effects use the new one.
        "setnthperkentryfunction" => {
            let code = num(args, 2, -1.0);
            if let (Some(p), true) = (form(args, 0), (0.0..=255.0).contains(&code)) {
                if let Some(e) = perk_entry_mut(g, p, num(args, 1, -1.0)) {
                    if let PerkEffect::EntryPoint(ep) = &mut e.effect {
                        ep.point = code as u8;
                    }
                }
            }
            zero
        }
        // `perk index text`: an entry point entry's button label, the activate choice's
        // prompt [documented: SetNthPerkEntryString, "Activate Prompt" for type 4].
        "setnthperkentrystring" => {
            if let Some(p) = form(args, 0) {
                let t = text(args, 2);
                if let Some(e) = perk_entry_mut(g, p, num(args, 1, -1.0)) {
                    if let PerkEffect::EntryPoint(ep) = &mut e.effect {
                        ep.button = Some(t);
                    }
                }
            }
            zero
        }
        // ---- the world
        // The regions the player stands in, in FormID order [documented: GetPlayerRegions;
        // the test as IsPlayerInRegion's, `functions::objects::player_in_region`;
        // hypothesis: the order].
        "getplayerregions" => {
            let mut ids: Vec<FormId> = g.data.regions.keys().copied().collect();
            ids.sort();
            let inside: Vec<XVal> = ids.into_iter().filter(|&r| crate::functions::objects::player_in_region(g, r).unwrap_or(false)).map(|r| XVal::Ref(r.0)).collect();
            let entries = inside.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // Event names for SetEventHandler (`SetEventHandler "OnCellEnter" MyUDF`
        // [documented]); called as functions they give 2 and 3 [observed: lu3 events]. The
        // events are sent by `Game::lu3_tick`.
        "oncellenter" => XVal::Num(2.0),
        "oncellexit" => XVal::Num(3.0),
        // `type depth taken cell`: GetRefsInCell's list [documented: GetCellRefs,
        // "deprecated by GetRefs/GetRefsInCell"]; the player's cell when none.
        "getcellrefs" => {
            let cell = form(args, 3).or_else(|| g.cell_of(FormId::PLAYER_REF));
            let list = g.ref_walk_list(cell, num(args, 0, 0.0) as i64, num(args, 1, 0.0) as i64, num(args, 2, 0.0) != 0.0);
            let entries = list.iter().enumerate().map(|(i, r)| (XKey::Num(i as f64), XVal::Ref(r.0))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // `name value [quest]`: SetVariable's [documented: "deprecated by SetVariable"].
        "setvariablefloat" => return crate::nvse::call(g, ctx, "setvariable", subject, args),
        // `item count health [base]`: the base container's entry with the unit's health,
        // a fraction 0-1, for the session [documented: BaseAddItemHealth, "NOT save
        // baked"; deprecated]. The base given, else the calling reference's.
        "baseadditemhealth" => {
            let (Some(item), Some(b)) = (form(args, 0), form(args, 3).or(subject).map(|f| base(g, f))) else { return Some(Ok(zero)) };
            let n = num(args, 1, 1.0).max(1.0) as i32;
            let h = num(args, 2, 1.0).clamp(0.0, 1.0) as f32;
            g.world.edit_base_entries(b, |e| {
                let mut x = fnv_sim::forms::Entry::new(item, n);
                x.extra.condition = (h < 1.0).then_some(h);
                e.push(x);
            });
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// Equips the unit of `item` of that health (points; below 0 any) and mods (`None` any),
/// with OnEquip as EquipItem's; `lock` keeps it on (noUnequip).
fn equip(g: &mut Game, a: FormId, item: FormId, health: f64, mods: Option<u8>, lock: bool) {
    // The carried unit whose health is within half a point of the one asked (health
    // points come back from GetEquippedData as condition × full health) [design].
    let full = crate::nvse::record_value(g, item, "DATA", "health").and_then(|v| v.num().ok()).unwrap_or(0.0);
    let condition = if health < 0.0 {
        None
    } else {
        let units: Vec<(f32, u8)> = g.world.inventory_mut(a).map(|inv| inv.groups().filter(|(i, _, _)| *i == item).map(|(_, e, _)| (e.condition_or_whole(), e.mods)).collect()).unwrap_or_default();
        let near = units.into_iter().filter(|(_, m)| mods.is_none_or(|x| x == *m)).find(|(c, _)| (f64::from(*c) * full - health).abs() < 0.5 || (full <= 0.0 && f64::from(*c) == health));
        match near {
            Some((c, _)) => Some(c),
            None => return,
        }
    };
    let pick = fnv_sim::Pick { condition, mods };
    let Ok(e) = g.world.equip_at(a, item, pick) else { return };
    if !e.consumed && lock {
        let _ = g.world.lock_equip(a, item, Some(fnv_sim::world::EquipLock::NoUnequip));
    }
    for u in e.unequipped {
        g.item_event(a, u, crate::game::event::ON_UNEQUIP, Some(fnv_script::vm::Value::Ref(a.0)));
    }
    g.equip_events(a, item, e.consumed);
    g.sync_actor_effects(a);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{EntryPointDef, PerkEntry};

    fn perk(entries: &[(u8, PerkEffect)]) -> PerkDef {
        let entries = entries.iter().enumerate().map(|(k, (rank, effect))| PerkEntry { rank: *rank, priority: 0, effect: effect.clone(), ordinal: k as u32 }).collect();
        PerkDef { entries, ..Default::default() }
    }

    fn point(p: u8) -> PerkEffect {
        PerkEffect::EntryPoint(EntryPointDef { point: p, ..Default::default() })
    }

    fn order(d: &PerkDef) -> Vec<usize> {
        (0..d.entries.len()).map(|i| nth_entry(d, i as f64).unwrap()).collect()
    }

    /// The perks the probes read [observed: lu3 perkentry; gaps4, jip2; L5.ext rerun, #2040].
    #[test]
    fn entries_go_by_rank_then_kind_then_reverse_record_order() {
        // Fast Shot: four entry points, entry 1 the third in the record.
        assert_eq!(order(&perk(&[(0, point(40)), (0, point(43)), (0, point(8)), (0, point(34))])), vec![3, 2, 1, 0]);
        // Small Frame: two abilities, then an entry point; abilities at 0 and 1.
        let small = perk(&[(0, PerkEffect::Ability(FormId(0))), (0, PerkEffect::Ability(FormId(0x135EC2))), (0, point(6))]);
        assert_eq!(order(&small)[2], 2);
        // Built to Last: its ability first.
        assert_eq!(order(&perk(&[(0, PerkEffect::Ability(FormId(0x137B00))), (0, point(0x44))])), vec![0, 1]);
        // Action Boy and Toughness: by rank.
        assert_eq!(order(&perk(&[(0, PerkEffect::Ability(FormId(1))), (1, PerkEffect::Ability(FormId(2)))])), vec![0, 1]);
        assert_eq!(order(&perk(&[(0, point(56)), (1, point(56))])), vec![0, 1]);
        assert_eq!(nth_entry(&small, 3.0), None);
        assert_eq!(nth_entry(&small, -1.0), None);
    }
}
