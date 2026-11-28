//! ShowOff NVSE functions of the 2026-10-30 batch (#1010): weapons and limbs, actor value
//! damage and names, ammo names, an aid item's consume sound, creature data, encounter
//! zones, form checks, game settings [documented: each page in the wiki export;
//! docs/spec-notes/ext-showoff.md §8]. Changes last for the session.

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

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// The Fists weapon a disarmed actor fights with [observed: FalloutNV.esm WEAP 000001F4,
/// EDID Fists].
const FISTS: FormId = FormId(0x1F4);

#[cfg(test)]
#[path = "w6d12_islimbcrippled_tests.rs"]
mod w6d12_islimbcrippled_tests;

/// The limbs' condition actor values in the pages' order: head, torso, left arm, right
/// arm, left leg, right leg, brain [documented: IsLimbCrippled; GetCrippledLimbsAsBitMask].
const LIMB_AVS: [&str; 7] = ["PerceptionCondition", "EnduranceCondition", "LeftAttackCondition", "RightAttackCondition", "LeftMobilityCondition", "RightMobilityCondition", "BrainCondition"];

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let zero = XVal::Num(0.0);
    // A weapon's type (DNAM animation type): melee is 2 and below [documented:
    // IsEquippedWeaponMelee, "check if the weapon's type is <= 2"].
    let weapon_type = |g: &Game, w: FormId| crate::nvse::record_value(g, w, "DNAM", "animation_type").and_then(|v| v.num().ok());
    // Whether each limb is crippled: its condition at or below the threshold [documented].
    let crippled = |g: &mut Game, a: FormId, threshold: f64| -> [bool; 7] {
        std::array::from_fn(|i| {
            let code = crate::functions::av_code(g, LIMB_AVS[i]).ok();
            code.is_some_and(|c| crate::functions::actor::actor_value(g, a, c).unwrap_or(100.0) <= threshold)
        })
    };
    let v = match name {
        // The equipped weapon's base, Fists when none [documented: GetEquippedWeapon].
        "getequippedweapon" => XVal::Ref(subject.and_then(|a| crate::functions::items::equipped_weapon(g, a)).unwrap_or(FISTS).0),
        // [documented: IsEquippedWeaponMelee, 1 with none (fists); IsEquippedWeaponRanged,
        // 0 with none; IsWeaponMelee, the argument else the calling reference's base].
        "isequippedweaponmelee" | "isequippedweaponranged" => {
            let w = subject.and_then(|a| crate::functions::items::equipped_weapon(g, a)).unwrap_or(FISTS);
            let melee = weapon_type(g, w).is_none_or(|t| t <= 2.0);
            yes(if name == "isequippedweaponmelee" { melee } else { !melee })
        }
        "isweaponmelee" => {
            let w = form(args, 0).or(subject).map(|f| base(g, f));
            yes(w.filter(|&w| g.data.sig(w) == Some(*b"WEAP")).and_then(|w| weapon_type(g, w)).is_some_and(|t| t <= 2.0))
        }
        // [documented: GetItemCanHaveHealth, "only true if the item has the Weapon or
        // Armor type"].
        "getitemcanhavehealth" => {
            let i = form(args, 0).or(subject).map(|f| base(g, f));
            yes(i.and_then(|i| g.data.sig(i)).is_some_and(|s| &s == b"WEAP" || &s == b"ARMO"))
        }
        // `[limb] [threshold]`: -1 or none for any limb [documented: IsLimbCrippled];
        // `[threshold]` [documented: GetCrippledLimbsAsBitMask, head 1 … brain 64;
        // GetNumCrippledLimbs]. The threshold defaults to 0.
        "islimbcrippled" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let limb = num(args, 0, -1.0) as i64;
            let c = crippled(g, a, num(args, 1, 0.0));
            yes(match limb {
                -1 => c.iter().any(|&x| x),
                0..=6 => c[limb as usize],
                _ => false,
            })
        }
        "getcrippledlimbsasbitmask" | "getnumcrippledlimbs" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let c = crippled(g, a, num(args, 0, 0.0));
            XVal::Num(if name == "getnumcrippledlimbs" {
                c.iter().filter(|&&x| x).count() as f64
            } else {
                f64::from(c.iter().enumerate().filter(|(_, &x)| x).map(|(i, _)| 1u32 << i).sum::<u32>())
            })
        }
        // `av`: the damage taken, positive [documented: GetActorValueDamage, alias
        // GetAVDamage], no more than the value had: DamageAV 200 on a limb of 100 reads
        // 100 [observed: showoff2].
        "getactorvaluedamage" | "getavdamage" => {
            let (Some(a), Some(code)) = (subject, args.first().and_then(|v| av(g, v))) else { return Some(Ok(zero)) };
            let d = crate::functions::actor::damage(g, a, code);
            let [b, m, _] = crate::functions::actor::actor_value_parts(g, a, code).unwrap_or([d, 0.0, d]);
            XVal::Num(d.min((b + m).max(0.0)))
        }
        // `code`: the actor value's script name [documented: GetActorValueName, 1 gives
        // "Confidence"; alias GetAVName; observed: showoff2, 25 PerceptionCondition, 76
        // DamageThreshold, no spaces].
        "getactorvaluename" | "getavname" => XVal::Str(args.first().and_then(|v| av(g, v)).map(script_av_name).unwrap_or_default()),
        // `[type] [ammo]`: 0 the full name (FULL), 1 the short name (ONAM), 2 the
        // abbreviation (QNAM) [documented: GetAmmoName; the records' subrecords:
        // xEdit wbDefinitionsFNV.pas AMMO]; SetAmmoName `name [type] [ammo]` for the
        // session.
        "getammoname" | "setammoname" => {
            let set = name == "setammoname";
            let at = usize::from(set);
            let Some(a) = form(args, at + 1).or(subject).map(|f| base(g, f)).filter(|&a| g.data.sig(g.unclone(a)) == Some(*b"AMMO")) else {
                return Some(Ok(if set { zero } else { XVal::Str(String::new()) }));
            };
            let sub = match num(args, at, 0.0) as i64 {
                1 => "ONAM",
                2 => "QNAM",
                _ => "FULL",
            };
            if set {
                let t = match args.first() {
                    Some(XVal::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                g.field_overrides.insert((a, format!("{sub}/")), XVal::Str(t));
                zero
            } else {
                // A name the record lacks reads "<no name>" [observed: showoff2, 10mm ammo's
                // abbreviation]. Check `a`'s own session override before falling back to the
                // authored record of `unclone(a)` so post-clone edits on the original stay
                // isolated from existing clones.
                if let Some(XVal::Str(s)) = g.field_overrides.get(&(a, format!("{sub}/"))) {
                    return Some(Ok(XVal::Str(s.clone())));
                }
                let orig = g.unclone(a);
                if let Some(s) = g.data.record_field(orig, sub, "").and_then(serde_json::Value::as_str) {
                    XVal::Str(s.to_string())
                } else if sub == "FULL" {
                    XVal::Str(g.name_overrides.get(&a).or_else(|| g.forms.saved_names.get(&a)).or_else(|| g.data.full_names.get(&orig)).cloned().unwrap_or_else(|| "<no name>".into()))
                } else {
                    XVal::Str("<no name>".into())
                }
            }
        }
        // [documented: GetIngestibleConsumeSound; SetIngestibleConsumeSound, true when
        // set]: ENIT's consume sound.
        "getingestibleconsumesound" => {
            let i = form(args, 0).or(subject).map(|f| base(g, f));
            XVal::Ref(i.and_then(|i| g.ingestible_def(i)).and_then(|d| d.consume_sound).map_or(0, |r| r.0))
        }
        "setingestibleconsumesound" => {
            let (Some(s), Some(i)) = (form(args, 0), form(args, 1).or(subject).map(|f| base(g, f))) else { return Some(Ok(zero)) };
            if g.data.sig(g.unclone(i)) != Some(*b"ALCH") {
                return Some(Ok(zero));
            }
            g.field_overrides.insert((i, "ENIT/sound_consume".into()), XVal::Ref(s.0));
            XVal::Num(1.0)
        }
        // `[creature]`: -1 for a form that is no creature [documented:
        // GetCreatureFootWeight (WNAM); GetCreatureTurningSpeed (TNAM)].
        "getcreaturefootweight" | "getcreatureturningspeed" => {
            let c = form(args, 0).or(subject).map(|f| base(g, f)).filter(|&c| g.data.sig(c) == Some(*b"CREA"));
            let sub = if name == "getcreaturefootweight" { "WNAM" } else { "TNAM" };
            XVal::Num(c.map_or(-1.0, |c| crate::nvse::record_value(g, c, sub, "").and_then(|v| v.num().ok()).unwrap_or(0.0)))
        }
        // `value [creature]`, at or above 0, true when set [documented:
        // SetCreatureFootWeight, SetCreatureTurningSpeed, SetCreatureBaseScale (BNAM),
        // SetCreatureReach (RNAM)].
        "setcreaturefootweight" | "setcreatureturningspeed" | "setcreaturebasescale" | "setcreaturereach" => {
            let v = num(args, 0, -1.0);
            let Some(c) = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&c| g.data.sig(c) == Some(*b"CREA")) else { return Some(Ok(zero)) };
            if v < 0.0 {
                return Some(Ok(zero));
            }
            let sub = match name {
                "setcreaturefootweight" => "WNAM",
                "setcreatureturningspeed" => "TNAM",
                "setcreaturebasescale" => "BNAM",
                _ => "RNAM",
            };
            g.field_overrides.insert((c, format!("{sub}/")), XVal::Num(if sub == "RNAM" { v.trunc() } else { v }));
            XVal::Num(1.0)
        }
        // `cell`: its encounter zone [documented: GetCellEncounterZone, alias GetCellZone].
        // SetCellEncounterZone's and SetWorldspaceEncounterZone's changes first
        // (`nvse_jg5::cell_zone`).
        "getcellencounterzone" | "getcellzone" => XVal::Ref(form(args, 0).and_then(|c| crate::nvse_jg5::cell_zone(g, c)).map_or(0, |z| z.0)),
        // `zone`: whether its cells respawn, so not "Never Resets" [documented:
        // GetZoneRespawns; the ECZN flag, encounter-zones.md §1], reading saved
        // `SetZoneRespawns` and session `SetZoneFlag` changes through `Game::zone`.
        "getzonerespawns" => yes(form(args, 0).and_then(|z| g.zone(z)).is_some_and(|d| !d.never_resets)),
        // [documented: IsBaseForm, a valid base form; IsReferenceCloned, a reference made
        // in play, mod index 0xFF; IsActorAlt, an actor reference or an NPC or creature
        // base].
        // A base form: returns 1 for authored base forms (WEAP, NPC_, CELL, WRLD, QUST,
        // FACT, FLST) and TempCloneForm 0xFF base forms, and 0 for Player, DocMitchellRef,
        // and doors [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9,
        // ShowOffNVSE 184, GSDocMitchellHouse (0x00103DF9), 10 official base masters,
        // `tools/capture/nvse/catalogs/wave6d1_is_base_form_tempclone.json` probe
        // `w6d1_35_showoff_is_base_form_and_fade_state`:
        // `base(w=1,cl=1,n=1,ce=1,wr=1,qu=1,fa=1,fl=1,doc=0,pc=0,dr=0)`;
        // no DLL, PDB, or third-party source consulted]. Rejecting 0xFF PlaceAtMe placed
        // refs and temporary inventory refs via `world.placed`, `temp_item`, and `clones`
        // follows [documented: IsBaseForm, a valid base form; design].
        "isbaseform" => yes(form(args, 0).is_some_and(|f| {
            g.world.placed.get(f).is_none()
                && g.temp_item(f).is_none()
                && (f.0 >> 24 != 0xFF || g.clones.contains_key(&f))
                && g.data.sig(g.unclone(f)).is_some_and(|s| !matches!(&s, b"REFR" | b"ACHR" | b"ACRE" | b"PGRE" | b"PMIS"))
        })),
        "isreferencecloned" => yes(subject.is_some_and(|r| r.0 >> 24 == 0xFF)),
        "isactoralt" => {
            let f = form(args, 0).or(subject).map(|f| base(g, f));
            yes(f.and_then(|f| g.data.sig(f)).is_some_and(|s| &s == b"NPC_" || &s == b"CREA"))
        }
        // `setting value`: a numeric setting changed by the value, 1 when done
        // [documented: ModNumericGameSetting].
        "modnumericgamesetting" => {
            // The name quoted or bare (`crate::nvse_misc::setting_name`).
            let n = crate::nvse_misc::setting_name(g, args, 0);
            let Some(now) = g.world.setting_f32(&n) else { return Some(Ok(zero)) };
            let new = f64::from(now) + num(args, 1, 0.0);
            let cctx = crate::functions::console::ConsoleCtx { on: subject, active_quest: g.ui.active_quest };
            yes(crate::functions::console::set_game_setting(g, &cctx, &[n, format!("{new}")]).is_ok())
        }
        // ShowOff's own debug prints [documented: GetShowOffDebugMode; SetShowOffDebugMode]:
        // kept, nothing more is printed.
        "getshowoffdebugmode" => yes(g.other.showoff_debug),
        "setshowoffdebugmode" => {
            g.other.showoff_debug = num(args, 0, 0.0) != 0.0;
            zero
        }
        _ => return None,
    };
    let _ = ctx;
    Some(Ok(v))
}

/// An actor value argument: its code, or its name.
fn av(g: &Game, v: &XVal) -> Option<u16> {
    match v {
        XVal::Num(c) => Some(*c as u16),
        XVal::Str(n) => crate::functions::av_code(g, n).ok(),
        _ => None,
    }
}

/// An actor value's script name: the table's exceptions, else its name without spaces.
fn script_av_name(code: u16) -> String {
    match fnv_script::table::SCRIPT_AV_NAMES.iter().find(|(c, _)| *c == code) {
        Some((_, n)) => n.to_string(),
        None => crate::functions::actor::AV_NAMES.get(code as usize).map(|n| n.replace(' ', "")).unwrap_or_default(),
    }
}
