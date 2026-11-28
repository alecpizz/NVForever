//! The JIP / LU functions the owner's Amata companion mod (AMFAmata.esp) uses that were
//! not built (docs/handoff/modlist-scan-2026-10-26.md) [documented: their pages,
//! JIP.xml and functions.xml].

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

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

impl Game<'_> {
    /// The EFIT effect entries of an ingestible, object effect or actor effect, in record
    /// order, with SetNthEffectTraitNumeric's changes: [magnitude, area, duration].
    fn effect_traits(&self, f: FormId) -> Vec<[f64; 3]> {
        let mut out: Vec<[f64; 3]> = self
            .data
            .record_fields
            .get(&self.unclone(f))
            .into_iter()
            .flatten()
            .filter(|(s, _)| s == b"EFIT")
            .map(|(_, v)| ["magnitude", "area", "duration"].map(|k| v.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0)))
            .collect();
        for (i, e) in out.iter_mut().enumerate() {
            for (t, x) in e.iter_mut().enumerate() {
                if let Some(v) = self.effect_overrides.get(&(f, i, t)) {
                    *x = *v;
                }
            }
        }
        out
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let v = match name {
        // [documented: ModBaseActorValue] `actorValue modifier [actorBase]`: the base
        // value moves by the modifier.
        "modbaseactorvalue" => {
            let a = form(args, 2).or(subject).unwrap_or(FormId(0));
            let code = match args.first() {
                Some(XVal::Num(x)) => Some(*x as u16),
                Some(XVal::Str(s)) => crate::functions::av_code(g, s).ok(),
                _ => None,
            };
            if let Some(code) = code {
                if let Err(e) = crate::functions::actor::shift_base(g, a, code, num(args, 1, 0.0)) {
                    return Some(Err(e));
                }
            }
            XVal::Num(0.0)
        }
        // [documented: SetHair; SetHairColor] the NPC base's hair and its colour (an int,
        // R + G×256 + B×65536 as GetHairColor packs it [observed]) for the session; the
        // look changes when the 3D is rebuilt.
        "sethair" => {
            let (Some(h), npc) = (form(args, 0), form(args, 1).or(subject).map(|f| base(g, f))) else { return Some(Ok(XVal::Num(0.0))) };
            let Some(npc) = npc else { return Some(Ok(XVal::Num(0.0))) };
            g.field_overrides.insert((npc, "HNAM/".into()), XVal::Ref(h.0));
            XVal::Num(1.0)
        }
        "sethaircolor" => {
            let npc = form(args, 1).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            let rgb = num(args, 0, 0.0) as u32;
            for (k, part) in [("red", rgb & 0xFF), ("green", (rgb >> 8) & 0xFF), ("blue", (rgb >> 16) & 0xFF)] {
                g.field_overrides.insert((npc, format!("HCLR/{k}")), XVal::Num(f64::from(part)));
            }
            XVal::Num(0.0)
        }
        // [documented: GetNthEffectTraitNumeric; SetNthEffectTraitNumeric] traits 0
        // magnitude, 1 area, 2 duration of the Nth effect entry.
        // The entry is the scripts' list's (`Game::effect_slot`, #2016): a record entry's
        // traits are its EFIT's, AddNewEffect's its own.
        "getntheffecttraitnumeric" | "setntheffecttraitnumeric" => {
            use crate::nvse_jip14::EffectSlot;
            let f = form(args, 0).unwrap_or(FormId(0));
            let (i, t) = (num(args, 1, -1.0) as i64, num(args, 2, -1.0) as i64);
            let slot = usize::try_from(i).ok().and_then(|i| g.effect_slot(f, i));
            let Some(t) = usize::try_from(t).ok().filter(|&t| t < 3) else { return Some(Ok(XVal::Num(0.0))) };
            let get = name == "getntheffecttraitnumeric";
            // Returns 0 [observed: gaps6, setting 77].
            let v = num(args, 3, 0.0).trunc();
            match slot {
                Some(EffectSlot::Own(r)) => {
                    let traits = g.effect_traits(f);
                    match traits.get(r) {
                        Some(x) if get => XVal::Num(x[t]),
                        Some(_) => {
                            g.effect_overrides.insert((f, r, t), v);
                            XVal::Num(0.0)
                        }
                        None => XVal::Num(0.0),
                    }
                }
                Some(EffectSlot::Added { back, at }) => {
                    if get {
                        XVal::Num(g.added_effect_trait(f, back, at, t).unwrap_or(0.0))
                    } else {
                        g.set_added_effect_trait(f, back, at, t, v);
                        XVal::Num(0.0)
                    }
                }
                None => XVal::Num(0.0),
            }
        }
        // [documented: GetValueAlt] the base value, ingestibles included (ENIT value).
        "getvaluealt" => {
            let f = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            let sub = if g.data.sig(g.unclone(f)) == Some(*b"ALCH") { "ENIT" } else { "DATA" };
            match crate::nvse::record_value(g, f, sub, "value") {
                Some(XVal::Num(x)) => XVal::Num(x),
                _ => XVal::Num(0.0),
            }
        }
        // [documented: GetCurrentAmmo] the ammo the actor's weapon holds.
        "getcurrentammo" => XVal::Ref(subject.and_then(|a| g.ammo_in_use(a)).map_or(0, |a| a.0)),
        // [documented: ToggleTeammateKillable; SetWheelDisabled; ToggleNoHealthReset]
        // flags kept for the session. A killable teammate dies outside Hardcore
        // (`functions::actor::essential`) [hypothesis: the companion wheel and JIP's
        // health reset do not read their flags yet].
        // Gives the state before [observed: jip-fifth, 1 then 0 on Doc give 0 then 1].
        "toggleteammatekillable" => {
            let a = subject.unwrap_or(FormId(0));
            let before = g.jip_flags.contains(&(a, "killable"));
            if num(args, 0, 0.0) != 0.0 {
                g.jip_flags.insert((a, "killable"));
            } else {
                g.jip_flags.remove(&(a, "killable"));
            }
            yes(before)
        }
        "setwheeldisabled" => {
            let a = subject.unwrap_or(FormId(0));
            if num(args, 0, 0.0) != 0.0 {
                g.jip_flags.insert((a, "no_wheel"));
            } else {
                g.jip_flags.remove(&(a, "no_wheel"));
            }
            XVal::Num(0.0)
        }
        "togglenohealthreset" => {
            if num(args, 0, 0.0) != 0.0 {
                g.jip_flags.insert((FormId(0), "no_health_reset"));
            } else {
                g.jip_flags.remove(&(FormId(0), "no_health_reset"));
            }
            XVal::Num(0.0)
        }
        // [documented: Update3D] the 3D is rebuilt [hypothesis: the engine redraws an
        // actor's look from the game state each time it loads; nothing to do here].
        "update3d" => XVal::Num(0.0),
        // [documented: IsSpellTargetAlt] the reference is under an effect of that form.
        "isspelltargetalt" => {
            let (Some(r), Some(s)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Num(0.0))) };
            g.sync_actor_effects(r);
            yes(g.effects.iter().any(|e| e.target == r && e.source == s))
        }
        // [documented: GetAllItemRefs] `[typeCode] [noNonPlayable] [noQuestItems]
        // [noEquipped]`: inventory references of the items, one per kind.
        "getallitemrefs" => {
            let c = subject.unwrap_or(FormId(0));
            let (code, no_np, no_q, no_eq) = (num(args, 0, 0.0) as i64, num(args, 1, 0.0) != 0.0, num(args, 2, 0.0) != 0.0, num(args, 3, 0.0) != 0.0);
            let items: Vec<(FormId, bool)> = g
                .world
                .inventory_mut(c)
                .map(|inv| inv.iter().filter(|(_, n)| *n > 0).map(|(i, _)| (i, inv.is_equipped(i))).collect())
                .unwrap_or_default();
            let mut refs = Vec::new();
            for (it, eq) in items {
                let sig = g.data.sig(g.unclone(it));
                let ty_ok = code == 0 || sig.and_then(|s| fnv_script::vm::form_type_code(&s)).is_some_and(|t| i64::from(t) == code);
                let np = g.is_item_non_playable(it);
                if ty_ok && !(no_np && np) && !(no_q && g.world.is_quest_item(it)) && !(no_eq && eq) {
                    refs.push(XVal::Ref(g.temp_ref(c, it).0));
                }
            }
            let entries = refs.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // [documented: GetItemRefCurrentHealth] `[0 health | 1 fraction | 2 base]`.
        "getitemrefcurrenthealth" => {
            let r = subject.unwrap_or(FormId(0));
            let (item, cond) = match g.temp_item(r) {
                Some((c, it)) => {
                    let cond = g.world.inventory_mut(c).ok().and_then(|inv| inv.groups().find(|(i, _, _)| *i == it).map(|(_, e, _)| e.condition_or_whole())).unwrap_or(1.0);
                    (it, f64::from(cond))
                }
                None => (base(g, r), 1.0),
            };
            let full = match crate::nvse::record_value(g, item, "DATA", "health") {
                Some(XVal::Num(x)) => x,
                _ => 0.0,
            };
            XVal::Num(match num(args, 0, 0.0) as i64 {
                1 => cond,
                2 => full,
                _ => full * cond,
            })
        }
        // [documented: GetMerchantContainer] the actor's merchant container reference.
        "getmerchantcontainer" => XVal::Ref(subject.and_then(|a| g.merchant_container(a)).map_or(0, |c| c.0)),
        // [documented: IsFleeing] fleeing from combat [hypothesis: the combat AI's flight;
        // flee packages not counted].
        "isfleeing" => yes(subject.is_some_and(|a| g.awareness.tactics.flights.contains_key(&a))),
        _ => return None,
    };
    Some(Ok(v))
}

impl Game<'_> {
    /// MessageBoxExAlt's callback, once its box has a button: the UDF runs with the
    /// button's index and the box closes [documented: MessageBoxExAlt].
    pub(crate) fn run_box_callback(&mut self) {
        let Some(b) = self.message_box.as_ref().filter(|_| self.box_callback.is_some()).and_then(|m| m.pressed) else { return };
        let Some(func) = self.box_callback.take() else { return };
        self.message_box = None;
        let ctx = crate::game::RunCtx::default();
        if let Err(error) = self.run_function(&func, None, vec![XVal::Num(f64::from(b))], None, ctx) {
            self.emit(crate::trace::EventKind::ScriptError { unit: "MessageBoxExAlt callback".into(), error });
        }
    }
}
