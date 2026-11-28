//! xNVSE's weapon-mod, faction, race, perk, ownership and input-list functions
//! (docs/handoff/xnvse-remaining.md, second pass) [documented: their pages].

use crate::game::Game;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn int(args: &[XVal], k: usize, dflt: i64) -> i64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x as i64,
        _ => dflt,
    }
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn num(v: Option<XVal>) -> XVal {
    match v {
        Some(XVal::Num(x)) => XVal::Num(x),
        _ => XVal::Num(0.0),
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    // The form at `k`, else the calling reference's base form.
    let item = |g: &Game, k: usize| form(args, k).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
    let field = |g: &Game, f: FormId, sub: &str, path: &str| crate::nvse::record_value(g, f, sub, path);
    let v = match name {
        // `GetWeaponItemMod slot [weapon]` (1–3): WMI1–3; the effect and values: DNAM
        // effect_modN, value_a_modN, value_b_modN [documented: their pages; the fields are
        // the schema's; observed 2026-11-24: `w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags`
        // (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
        // `imod(4.00,0.40)` for `Weap10mmPistol` (`0x434F`) slots 1 and 2].
        "getweaponitemmod" | "getweaponitemmodeffect" | "getweaponitemmodvalue1" | "getweaponitemmodvalue2" => {
            let slot = int(args, 0, 0);
            if !(1..=3).contains(&slot) {
                return Some(Ok(if name == "getweaponitemmod" { XVal::Ref(0) } else { XVal::Num(0.0) }));
            }
            let w = item(g, 1);
            match name {
                "getweaponitemmod" => match field(g, w, &format!("WMI{slot}"), "") {
                    Some(XVal::Ref(r)) => XVal::Ref(r),
                    _ => XVal::Ref(0),
                },
                "getweaponitemmodeffect" => num(field(g, w, "DNAM", &format!("effect_mod{slot}"))),
                "getweaponitemmodvalue1" => num(field(g, w, "DNAM", &format!("value_a_mod{slot}"))),
                _ => num(field(g, w, "DNAM", &format!("value_b_mod{slot}"))),
            }
        }
        // The base form's factions and ranks as arrays, in GetBaseNthFaction's order
        // [hypothesis: the same order, from the record's last SNAM back].
        "getbasefactions" | "getbaseranks" => {
            let a = g.data.base_template_source(item(g, 0), |d| d.template_factions);
            let mut f = g.data.actors.get(&a).map(|d| d.factions.clone()).unwrap_or_default();
            f.reverse();
            let items = f.into_iter().map(|(x, r)| if name == "getbasefactions" { XVal::Ref(x.0) } else { XVal::Num(f64::from(r)) }).collect();
            list(g, items)
        }
        // [documented: GetActiveFactions; GetActiveRanks] what the actor belongs to now,
        // with AddToFaction and the rest.
        "getactivefactions" | "getactiveranks" => {
            let a = form(args, 0).or(subject).unwrap_or(FormId(0));
            // In the base factions' reversed order [observed: nvse-second, Sunny's [0] is
            // her record's last SNAM].
            let mut fs = crate::functions::actor::factions_of(g, a);
            fs.reverse();
            let items = fs
                .into_iter()
                .map(|f| if name == "getactivefactions" { XVal::Ref(f.0) } else { XVal::Num(f64::from(crate::functions::actor::faction_rank(g, a, f))) })
                .collect();
            list(g, items)
        }
        // [documented: GetRaceName; GetRaceHairs; GetRaceEyes].
        "getracename" => {
            let a = form(args, 0).or(subject).unwrap_or(FormId(0));
            let race = g.actor_race(a).or_else(|| g.data.actors.get(&base(g, a)).and_then(|d| d.race));
            XVal::Str(race.and_then(|r| g.data.full_names.get(&r).cloned()).unwrap_or_default())
        }
        "getracehairs" | "getraceeyes" => {
            let r = form(args, 0).unwrap_or(FormId(0));
            let (sub, code) = if name == "getracehairs" { ("HNAM", 0) } else { ("ENAM", 1) };
            // GenericAddForm's and the others' changes first (`crate::nvse_lists`).
            let ids: Vec<XVal> = match g.other.generic.get(&(code, r)) {
                Some(l) => l.iter().map(|f| XVal::Ref(f.0)).collect(),
                None => g
                    .data
                    .record_field(r, sub, "")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).filter_map(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).map(XVal::Ref).collect())
                    .unwrap_or_default(),
            };
            list(g, ids)
        }
        // [documented: GetPerkRank; GetAltPerkRank] the player's rank of a perk, 0 without
        // it, -1 with no perk; the companion form: whether the teammates have it.
        "getperkrank" | "getaltperkrank" => match form(args, 0) {
            None => XVal::Num(-1.0),
            Some(p) if name == "getperkrank" => XVal::Num(f64::from(g.player.perks.get(&p).copied().unwrap_or(0))),
            Some(p) => XVal::Num(f64::from(u8::from(g.player.teammate_perks.contains(&p)))),
        },
        // [documented: GetFirstRefForItem; GetNextRefForItem] the container's inventory
        // references of the item: one per kind here, so Next gives none [hypothesis].
        "getfirstrefforitem" => {
            let it = form(args, 0).unwrap_or(FormId(0));
            let c = subject.unwrap_or(FormId(0));
            let has = g.world.inventory_mut(c).map(|inv| inv.iter().any(|(i, n)| i == it && n > 0)).unwrap_or(false);
            XVal::Ref(if has { g.temp_ref(c, it).0 } else { 0 })
        }
        "getnextrefforitem" => XVal::Ref(0),
        // [documented: IsOwned; HasOwnership] whether an actor owns a reference: the
        // reference's live owner (SetOwnership/ClearOwnership/SetOwnershipTemp/XOWN) or
        // its cell's, the NPC itself or a faction it is in (at the live required rank
        // SetOwningFactionRequiredRank/XRNK asks).
        "isowned" | "hasownership" => {
            let (r, actor) = if name == "isowned" { (subject, form(args, 0)) } else { (form(args, 0), subject) };
            let (Some(r), Some(actor)) = (r, actor) else { return Some(Ok(XVal::Num(0.0))) };
            let owns = g.ownership(r).is_some_and(|o| g.counts_as_owner(actor, &o));
            XVal::Num(f64::from(u8::from(owns)))
        }
        // [documented: GetPressedKeys; GetDisabledKeys] scancodes as arrays.
        "getpressedkeys" | "getdisabledkeys" => {
            let codes: Vec<u16> = if name == "getpressedkeys" {
                let ignore = int(args, 0, 0) == 2;
                g.input.held.iter().copied().filter(|&c| ignore || !g.input.key_disabled(c)).collect()
            } else {
                g.input.blocked().into_iter().collect()
            };
            let items = codes.into_iter().map(|c| XVal::Num(f64::from(c))).collect();
            list(g, items)
        }
        // [documented: IsControl] 1 when a game control uses the key, 2 for a custom
        // control (SetIsControl).
        "iscontrol" => {
            let code = u16::try_from(int(args, 0, -1)).ok();
            XVal::Num(match code {
                Some(c) if g.input.is_bound(c) => 1.0,
                Some(c) if g.other.custom_controls.contains(&c) => 2.0,
                _ => 0.0,
            })
        }
        // [documented: GetBasePackages; observed: tmpl2 (#1057)] GetNthPackage's list as an
        // array.
        "getbasepackages" => {
            let b = item(g, 0);
            let src = g.data.base_template_source(b, |d| d.template_packages);
            let ps = g.data.actors.get(&src).map(|d| d.packages.clone()).unwrap_or_default();
            // AddPackageAt's and RemovePackageAt's list first.
            let ps = g.other.package_lists.get(&b).cloned().unwrap_or(ps);
            let items = ps.into_iter().map(|p| XVal::Ref(p.0)).collect();
            list(g, items)
        }
        // [documented: GetBaseSpellListSpells] the record's actor effects (SPLO), in record
        // order; the Use Actor Effect List template's [observed: tmpl3 (#1057), four
        // creatures read their template's list, one with none of its own].
        "getbasespelllistspells" => {
            let b = item(g, 0);
            let src = g.data.actors.get(&g.data.base_template_source(b, |d| d.template_spells));
            // GenericAddForm's and the others' changes first (`crate::nvse_lists`).
            let items = match g.other.generic.get(&(6, b)) {
                Some(l) => l.iter().map(|s| XVal::Ref(s.0)).collect(),
                None => src.map(|d| d.spells.iter().map(|s| XVal::Ref(s.0)).collect()).unwrap_or_default(),
            };
            list(g, items)
        }
        // [documented: GetEquipmentSlotsMask; GetEquipmentBipedMask; observed: nvse-third,
        // T-45d 40, metal armor 128, leather 0; slots 4 for all three] the BMDT biped flags;
        // the "equipment flags" BMDT's general flags.
        "getequipmentslotsmask" | "getequipmentbipedmask" => {
            let path = if name == "getequipmentslotsmask" { "biped_flags" } else { "general_flags" };
            num(field(g, item(g, 0), "BMDT", path))
        }
        // [documented: GetObjectEffect] the item's object effect (EITM).
        "getobjecteffect" => XVal::Ref(g.object_effect(item(g, 0)).map_or(0, |e| e.0)),
        // [documented: GetNameOfClass] the actor's class's name.
        "getnameofclass" => {
            let class = form(args, 0).or(subject).and_then(|f| g.traits_class(f));
            XVal::Str(class.and_then(|c| g.data.full_names.get(&c).cloned()).unwrap_or_default())
        }
        // [documented: IsPlayerSwimming].
        "isplayerswimming" => XVal::Num(f64::from(u8::from(g.motion(FormId::PLAYER_REF).swimming))),
        // [documented: GetDialogueTarget; GetDialogueSubject; GetDialogueSpeaker; observed:
        // nvse-third, all none without a conversation] the player's conversation: the actor
        // in it [hypothesis: the player's partner stands for target, subject and speaker
        // alike during one].
        "getdialoguetarget" | "getdialoguesubject" | "getdialoguespeaker" => XVal::Ref(g.dialogue.as_ref().map_or(0, |c| c.actor.0)),
        _ => return None,
    };
    Some(Ok(v))
}
