//! JohnnyGuitar NVSE, batch 4 (#1007): idle markers' animations and traits, regions'
//! weather lists, race flags and faction members [documented: each page in the wiki
//! export; docs/spec-notes/ext-jg4.md]. Setters change the record for the session, read
//! by the getters (and, for idle markers, by the engine's idles).

use crate::data::{RegionWeather, RegionWeathers};
use crate::game::{Game, RunCtx};
use crate::package::IdleCollection;
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

/// A whole number at `k`, else none.
fn index(args: &[XVal], k: usize) -> Option<i64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.fract() == 0.0 => Some(*x as i64),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// An idle marker's collection as it stands now: a script's changes for the session
/// (`nvse_jg4`), else the record's (IDLM). The engine's idles read this.
pub fn idle_marker<'a>(g: &'a Game, marker: FormId) -> Option<&'a IdleCollection> {
    g.other.idle_markers.get(&marker).or_else(|| g.data.idle_markers.get(&marker))
}

/// A region's weather block as it stands now: a script's changes for the session, else
/// the record's (REGN RDAT type 3 + RDWT) in the game's order ([`game_order`]).
pub fn region_weathers(g: &Game, region: FormId) -> Option<RegionWeathers> {
    match g.other.region_weathers.get(&region) {
        Some(w) => Some(w.clone()),
        None => g.data.regions.get(&region)?.weathers.as_ref().map(game_order),
    }
}

/// The record's weathers in the order the game lists them: by chance, highest first,
/// weathers of the same chance in the opposite of the record's order [observed: jg4
/// regn.weathers.nv (100, 100) and regn.weathers.megaton (0, 0) listed reversed,
/// regn.weathers.urban (85, 15) in record order; hypothesis: the rule that joins them].
fn game_order(w: &RegionWeathers) -> RegionWeathers {
    let mut out = w.clone();
    out.list.reverse();
    out.list.sort_by_key(|a| std::cmp::Reverse(a.chance));
    out
}

/// The idle marker's collection to change, copied from the record on the first change.
fn marker_mut<'a>(g: &'a mut Game, marker: FormId) -> Option<&'a mut IdleCollection> {
    if !g.other.idle_markers.contains_key(&marker) {
        let c = g.data.idle_markers.get(&marker)?.clone();
        g.other.idle_markers.insert(marker, c);
    }
    g.other.idle_markers.get_mut(&marker)
}

/// The region's weather block to change, copied from the record on the first change. A
/// region without a weather block gains none: AddRegionWeather on one changes nothing
/// [observed: jg4 regn.add.dc01].
fn weathers_mut<'a>(g: &'a mut Game, region: FormId) -> Option<&'a mut RegionWeathers> {
    if !g.other.region_weathers.contains_key(&region) {
        let w = region_weathers(g, region)?;
        g.other.region_weathers.insert(region, w);
    }
    g.other.region_weathers.get_mut(&region)
}

fn list(g: &mut Game, items: impl Iterator<Item = XVal>) -> XVal {
    let entries = items.enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let of_sig = |g: &Game, f: Option<FormId>, sig: &[u8; 4]| f.filter(|&f| g.data.sig(f) == Some(*sig));
    let v = match name {
        // `marker`: its idles (IDLA) as an array [documented: GetIdleMarkerAnimations].
        "getidlemarkeranimations" => {
            let idles: Vec<FormId> = form(args, 0).and_then(|m| idle_marker(g, m)).map(|c| c.idles.clone()).unwrap_or_default();
            list(g, idles.into_iter().map(|i| XVal::Ref(i.0)))
        }
        // `marker trait`: 1 the flags (1 run in sequence, 4 do once), 2 the idle timer,
        // 3 the number of animations [documented: GetIdleMarkerTraitNumeric].
        "getidlemarkertraitnumeric" => {
            let Some(c) = form(args, 0).and_then(|m| idle_marker(g, m)) else { return Some(Ok(zero)) };
            let raw = form(args, 0).and_then(|m| g.other.idle_marker_flags.get(&m)).copied();
            XVal::Num(match index(args, 1) {
                Some(1) => f64::from(raw.unwrap_or(u8::from(c.run_in_sequence) | u8::from(c.do_once) << 2)),
                Some(2) => f64::from(c.timer),
                Some(3) => c.idles.len() as f64,
                _ => 0.0,
            })
        }
        // `marker trait value`: 1 the flags, 2 the timer; the count is not set here
        // [documented: SetIdleMarkerTraitNumeric and its note].
        "setidlemarkertraitnumeric" => {
            let (t, v) = (index(args, 1), num(args, 2, 0.0));
            let mut raw = None;
            if let Some((m, c)) = form(args, 0).and_then(|m| Some((m, marker_mut(g, m)?))) {
                match t {
                    // The flags kept as a whole byte, read back as set [observed: jg4
                    // idlm.settrait.hunter.1.2 read 2]; the marker plays by bits 0 and 2.
                    Some(1) => {
                        let bits = (v as i64).clamp(0, 255) as u8;
                        (c.run_in_sequence, c.do_once) = (bits & 1 != 0, bits & 4 != 0);
                        raw = Some((m, bits));
                    }
                    // A negative timer is kept [observed: jg4 idlm.settrait.hunter.2.neg
                    // read -1]; one that is not finite after the cast is refused: an
                    // endless one would stop the marker's idles for the session (#1333
                    // review; see docs/spec-notes/ext-jg4.md).
                    Some(2) if (v as f32).is_finite() => c.timer = v as f32,
                    _ => {}
                }
            }
            if let Some((m, bits)) = raw {
                g.other.idle_marker_flags.insert(m, bits);
            }
            zero
        }
        // `marker index idle`: the entry at the index replaced [documented:
        // SetIdleMarkerAnimation], by any form, an idle or not [observed: jg4
        // idlm.setanim.hunter.notidle, a race listed]; an index past the list changes
        // nothing (see docs/spec-notes/ext-jg4.md).
        "setidlemarkeranimation" => {
            let idle = form(args, 2);
            let at = index(args, 1).and_then(|i| usize::try_from(i).ok());
            if let (Some(m), Some(idle), Some(at)) = (form(args, 0), idle, at) {
                if idle_marker(g, m).is_some_and(|c| at < c.idles.len()) {
                    if let Some(c) = marker_mut(g, m) {
                        c.idles[at] = idle;
                    }
                }
            }
            zero
        }
        // `marker array`: the idles become the array's, in its order; the count follows
        // [documented: SetIdleMarkerAnimations and the note on the count]. Entries that
        // are not idles are left out (see docs/spec-notes/ext-jg4.md).
        "setidlemarkeranimations" => {
            let id = match args.get(1) {
                Some(XVal::Array(id)) => Some(*id),
                Some(XVal::Num(x)) if *x >= 1.0 => Some(*x as u32),
                _ => None,
            };
            let rows: Option<Vec<XVal>> = id.and_then(|id| g.ext.array(id)).map(|a| a.entries.iter().map(|(_, v)| v.clone()).collect());
            if let (Some(m), Some(rows)) = (form(args, 0), rows) {
                let idles: Vec<FormId> = rows.iter().filter_map(|v| of_sig(g, form(std::slice::from_ref(v), 0), b"IDLE")).collect();
                if let Some(c) = marker_mut(g, m) {
                    c.idles = idles;
                }
            }
            zero
        }
        // `region`: the weathers of its weather block, in record order [documented:
        // GetRegionWeathers].
        "getregionweathers" => {
            let ws: Vec<FormId> = form(args, 0).and_then(|r| region_weathers(g, r)).map(|w| w.list.iter().map(|e| e.weather).collect()).unwrap_or_default();
            list(g, ws.into_iter().map(|w| XVal::Ref(w.0)))
        }
        // `region index`: the entry's weather, chance or global [documented:
        // GetNthRegionWeatherType, …Chance, …Global].
        // Index 0 reads none and 0, whatever the list [observed: jg4 regn.nth.nv.0,
        // regn.nth.megaton.0, regn.add.gs.dup (chance 0 of a weather at 100)]; 1 on reads
        // the entry at that place in GetRegionWeathers' order [observed: jg4 regn.nth.nv.1,
        // regn.nth.megaton.1, regn.nth.urban.1, regn.add.gs].
        "getnthregionweathertype" | "getnthregionweatherchance" | "getnthregionweatherglobal" => {
            let e = form(args, 0)
                .and_then(|r| region_weathers(g, r))
                .and_then(|w| w.list.get(usize::try_from(index(args, 1).filter(|&i| i != 0)?).ok()?).copied());
            match name {
                "getnthregionweatherchance" => XVal::Num(e.map_or(0.0, |e| f64::from(e.chance))),
                "getnthregionweathertype" => XVal::Ref(e.map_or(0, |e| e.weather.0)),
                _ => XVal::Ref(e.and_then(|e| e.global).map_or(0, |g| g.0)),
            }
        }
        // `region weather` [documented: IsWeatherInRegion].
        "isweatherinregion" => {
            let w = form(args, 1);
            yes(w.is_some() && form(args, 0).and_then(|r| region_weathers(g, r)).is_some_and(|ws| ws.list.iter().any(|e| Some(e.weather) == w)))
        }
        // `region weather chance [global]` [documented: AddRegionWeather]: listed first
        // [observed: jg4 regn.add.gs]; a weather already listed is not listed twice and
        // keeps its entry [observed: jg4 regn.add.gs.dup].
        "addregionweather" => {
            let weather = of_sig(g, form(args, 1), b"WTHR");
            let global = of_sig(g, form(args, 3), b"GLOB");
            let chance = num(args, 2, 0.0).clamp(0.0, 100.0) as i32;
            if let (Some(r), Some(weather)) = (of_sig(g, form(args, 0), b"REGN"), weather) {
                if let Some(ws) = weathers_mut(g, r) {
                    if !ws.list.iter().any(|e| e.weather == weather) {
                        ws.list.insert(0, RegionWeather { weather, chance, global });
                    }
                }
            }
            zero
        }
        // `region weather` [documented: RemoveRegionWeather]; `region` [documented:
        // ClearRegionWeathers]: every entry but the first goes [observed: jg4
        // regn.clear.megaton kept its first weather].
        "removeregionweather" | "clearregionweathers" => {
            let w = form(args, 1);
            if let Some(ws) = form(args, 0).and_then(|r| weathers_mut(g, r)) {
                if name == "removeregionweather" {
                    ws.list.retain(|e| Some(e.weather) != w);
                } else {
                    ws.list.truncate(1);
                }
            }
            zero
        }
        // `region [flag]`: the weather block's override flag (RDAT flags bit 0)
        // [documented: Get/SetRegionWeatherOverride; the bit:
        // `fnv_world::sky::REGION_WEATHER_OVERRIDE`].
        "getregionweatheroverride" => yes(form(args, 0).and_then(|r| region_weathers(g, r)).is_some_and(|w| w.flags & fnv_world::sky::REGION_WEATHER_OVERRIDE != 0)),
        "setregionweatheroverride" => {
            let on = num(args, 1, 0.0) != 0.0;
            if let Some(ws) = form(args, 0).and_then(|r| weathers_mut(g, r)) {
                ws.flags = if on { ws.flags | fnv_world::sky::REGION_WEATHER_OVERRIDE } else { ws.flags & !fnv_world::sky::REGION_WEATHER_OVERRIDE };
            }
            zero
        }
        // `region [priority]`: RDAT's priority byte [documented: Get/
        // SetRegionWeatherPriority]; a priority outside 0-255 changes nothing [observed:
        // jg4 regn.setpriority.gs.300 and .neg kept 100].
        "getregionweatherpriority" => XVal::Num(form(args, 0).and_then(|r| region_weathers(g, r)).map_or(0.0, |w| f64::from(w.priority))),
        "setregionweatherpriority" => {
            let p = num(args, 1, 0.0);
            if (0.0..=255.0).contains(&p) {
                if let Some(ws) = form(args, 0).and_then(|r| weathers_mut(g, r)) {
                    ws.priority = p as u8;
                }
            }
            zero
        }
        // `race flag on`: 0 playable, 2 child (RACE DATA flags), read by GetRaceFlag
        // [documented: SetRaceFlag's table]; the other bits are set as well [observed: jg4
        // race.set.testqa.1.1 and .3.1]; bits 0-31 (see docs/spec-notes/ext-jg4.md).
        "setraceflag" => {
            let (Some(race), Some(bit @ 0..=31)) = (of_sig(g, form(args, 0), b"RACE"), index(args, 1)) else { return Some(Ok(zero)) };
            let flags = match crate::nvse::record_value(g, race, "DATA", "flags") {
                Some(XVal::Num(x)) => x as u64,
                _ => 0,
            };
            let flags = if num(args, 2, 0.0) != 0.0 { flags | 1 << bit } else { flags & !(1 << bit) };
            g.field_overrides.insert((race, "DATA/flags".into()), XVal::Num(flags as f64));
            zero
        }
        // `faction [rank]`: the base actors whose factions hold the faction, at that rank
        // when one is given [documented: GetFactionMembers]. The factions are the ones the
        // actor has, its template's when the template supplies them [observed: jg4
        // fact.members.mercfac listed the 16 mercenaries that take NVMercenaryMale's]; only
        // -1 (or omitted) means any rank, while -2 and -5 return 0 members [observed:
        // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, clean-room
        // script probe jg_11_faction_members_neg_rank: 5,0,0; no DLL or source consulted].
        // In the game's order: the NPC_ records, then the CREA records, each in the order
        // of the plugins' records [observed: jg4 fact.members.militia, GSCheyenne (CREA)
        // last; hypothesis: the order across plugins].
        "getfactionmembers" => {
            let rank = index(args, 1).filter(|&k| k != -1);
            let mut members: Vec<FormId> = match form(args, 0) {
                Some(f) => g
                    .data
                    .actors
                    .keys()
                    .filter(|&&id| {
                        let src = g.data.base_template_source(id, |a| a.template_factions);
                        let facs = g.data.actors.get(&src).map_or(&[][..], |a| &a.factions[..]);
                        facs.iter().any(|&(af, r)| af == f && rank.is_none_or(|k| i64::from(r) == k))
                    })
                    .copied()
                    .collect(),
                None => Vec::new(),
            };
            members.sort_by_key(|m| {
                let creature = g.data.actors.get(m).is_some_and(|a| a.sig != *b"NPC_");
                (creature, g.data.actor_order.get(m).copied().unwrap_or((u8::MAX, u32::MAX)), m.0)
            });
            list(g, members.into_iter().map(|m| XVal::Ref(m.0)))
        }
        _ => return None,
    };
    let _ = (ctx, subject);
    Some(Ok(v))
}
