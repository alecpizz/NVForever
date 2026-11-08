//! ShowOff NVSE functions of the 2026-11-06 batch (#1010): the equipped-item family, the
//! seeded randomizer, the aux timers' start and update handlers, temporary faction
//! relations, a reference's radiation, level-up points, challenge progress and small
//! getters [documented: each page in the wiki export; docs/spec-notes/ext-showoff.md §6].
//! Changes last for the session.

use crate::game::{Game, RunCtx};
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

/// An unsigned argument (a seed, RandSeeded's bounds): whole, and at most 2^31 - 1
/// [observed: showoff3 so3.seed.big, SetRandomizerSeed 4294967295 reads 2147483647 and
/// draws as seed 2147483647 does; so3.seed.range.uint, a maximum of 4294967294 draws half
/// the raw numbers].
fn uint(args: &[XVal], k: usize) -> u32 {
    num(args, k, 0.0).trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64 as u32
}

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: impl Iterator<Item = XVal>) -> XVal {
    let entries = items.enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// The seed RandSeeded and its siblings start from when none was set: 5489
/// [documented: GetRandomPercentSeeded, RandSeeded, SetRandomizerSeed, "default seed
/// (value of 5489)"]. 5489 is the published default seed of the Mersenne Twister
/// MT19937, and a draw maps onto `min..=max` as `min + floor(x * (max - min + 1) / 2^32)`,
/// a draw whose product's low 32 bits fall under 2^32 mod the span taken again
/// [observed: showoff3 so3.seed.default 82 14 91 84 13, so3.seed.20.twice, so3.seed.advance,
/// so3.seed.range.wide and .bits: MT19937's outputs for those seeds scaled so].
pub const DEFAULT_SEED: u32 = 5489;

/// MT19937 (Matsumoto and Nishimura, 1998): the 32-bit Mersenne Twister, the standard
/// C++ library's `std::mt19937` [documented: ISO C++ [rand.predef], its parameters and the
/// check that the 10000th output of a default-constructed one is 4123659995].
#[derive(Debug, Clone)]
pub struct Mt19937 {
    state: Box<[u32; 624]>,
    at: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Mt19937 {
        let mut state = Box::new([0u32; 624]);
        state[0] = seed;
        for i in 1..624 {
            state[i] = 1_812_433_253u32.wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30)).wrapping_add(i as u32);
        }
        Mt19937 { state, at: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.at >= 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7FFF_FFFF);
                let mut v = self.state[(i + 397) % 624] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= 0x9908_B0DF;
                }
                self.state[i] = v;
            }
            self.at = 0;
        }
        let mut y = self.state[self.at];
        self.at += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680;
        y ^= (y << 15) & 0xEFC6_0000;
        y ^ (y >> 18)
    }
}

/// One mod's seed: the seed and the generator at its place in the sequence.
#[derive(Debug, Clone)]
pub struct Seeded {
    pub seed: u32,
    pub rng: Mt19937,
}

impl Seeded {
    fn new(seed: u32) -> Seeded {
        Seeded { seed, rng: Mt19937::new(seed) }
    }
}

/// The equip slots a filter mask names: bits 0–19, the slots of GetEquippedObject (0
/// head … 5 weapon … 19 body addon 3) [documented: GetEquippedItemsAsBitMask's example,
/// "1 << 5" is the weapon; GetBaseEquippedWeight's, 12 is the upper body and left hand].
const SLOT_BITS: u32 = 0xF_FFFF;

/// The lowest slot an item takes, the order GetEquippedItems lists them in [observed:
/// showoff3 so3.items.allslots: the Vault 21 jumpsuit (upper body), the Pip-Boy glove (left
/// hand), the Pip-Boy].
fn first_slot(g: &Game, item: FormId) -> u32 {
    slots_of(g, item).trailing_zeros()
}

/// The equip slots an item takes: a weapon slot 5, armour its BMDT biped flags; none for
/// any other item (ammo, aid).
fn slots_of(g: &Game, item: FormId) -> u32 {
    match g.data.sig(item) {
        Some(s) if &s == b"WEAP" => 1 << 5,
        Some(s) if &s == b"ARMO" => match crate::nvse::record_value(g, item, "BMDT", "biped_flags") {
            Some(XVal::Num(b)) => (b as u64 as u32) & SLOT_BITS,
            _ => 0,
        },
        _ => 0,
    }
}

/// The items `actor` has equipped that a ShowOff filter mask picks, each once with its
/// unit's extra data: with no slot bits, every playable item in a slot; with slot bits,
/// every item in one of those slots, playable or not [documented: GetEquippedItems,
/// "If flags is omitted / set to 0, the function will check for every playable equipped
/// item"; GetCalculatedEquippedWeight's example, unplayable items in the named slots
/// "are still counted"; "will not return duplicates"]. The mask's further filters are on
/// a template page the export lacks, so bits 20 and up are ignored [hypothesis]. Items
/// without a slot (ammo) are not equipped items here [hypothesis: the functions count by
/// slot, as the duplicates note says].
fn equipped(g: &mut Game, actor: FormId, mask: u32) -> Vec<(FormId, fnv_sim::inventory::Extra)> {
    let forms = g.world.forms;
    let groups: Vec<(FormId, fnv_sim::inventory::Extra)> = match g.world.get(actor).ok().and_then(|s| s.inventory.as_ref()) {
        Some(inv) => inv.groups().filter(|x| x.1.equipped).map(|x| (x.0, x.1)).collect(),
        None => return Vec::new(),
    };
    let mut out: Vec<(FormId, fnv_sim::inventory::Extra)> = Vec::new();
    for (item, extra) in groups {
        if out.iter().any(|x| x.0 == item) {
            continue;
        }
        let slots = slots_of(g, item);
        if slots == 0 {
            continue;
        }
        let pick = if mask & SLOT_BITS == 0 { !forms.item(item).is_some_and(|i| i.non_playable) } else { slots & mask & SLOT_BITS != 0 };
        if pick {
            out.push((item, extra));
        }
    }
    out.sort_by_key(|x| first_slot(g, x.0));
    out
}

/// Where a reference is: its cell and position.
fn place(g: &Game, r: FormId) -> Option<(Option<FormId>, [f32; 3])> {
    g.world.peek(r).map(|s| (s.cell, s.pos)).or_else(|| g.world.placed.get(r).map(|p| (p.cell, p.pos)))
}

/// The 3-D distance between two references in the same space, `None` across spaces.
fn distance(g: &Game, a: FormId, b: FormId) -> Option<f64> {
    let ((c1, p1), (c2, p2)) = (place(g, a)?, place(g, b)?);
    let space = |c: Option<FormId>| c.map(|c| g.world.placed.world_of(c).unwrap_or(c));
    if c1.is_none() || space(c1) != space(c2) {
        return None;
    }
    Some((0..3).map(|i| (f64::from(p1[i]) - f64::from(p2[i])).powi(2)).sum::<f64>().sqrt())
}

/// The easing curves by ApplyEasingAlt's codes [documented: Sine 0, Quad 1, Cubic 2,
/// Quart 3, Quint 4, Circ 5, Expo 6, Back 7, Elastic 8, Bounce 9], by the names
/// ApplyEasing takes (`nvse_plugins::ease`).
const EASINGS: [&str; 10] = ["sine", "quad", "cubic", "quart", "quint", "circ", "expo", "back", "elastic", "bounce"];

impl Game<'_> {
    /// The seed of the mod whose script is running (`OtherState::plugin`, "" for the
    /// console), made with the default seed when none was set [documented: RandSeeded,
    /// "If no seed is set for the calling mod, this first generates a default seed"].
    fn mod_seed(&mut self) -> &mut Seeded {
        let m = self.other.plugin.to_ascii_lowercase();
        self.showoff.seeds.entry(m).or_insert_with(|| Seeded::new(DEFAULT_SEED))
    }

    /// The start handlers of an aux timer that was (re)started: each gets the timer's
    /// name and its owner [documented: SetOnAuxTimerStartHandler].
    pub(crate) fn aux_timer_started(&mut self, owner: FormId, name: &str) {
        let hs: Vec<XVal> = self.showoff.timer_starts.iter().filter(|(_, n, o)| n == name && *o == owner).map(|(f, _, _)| f.clone()).collect();
        for f in hs {
            let ctx = RunCtx { self_ref: Some(owner), ..RunCtx::default() };
            if let Err(error) = self.run_function(&f, Some(fnv_script::vm::Value::Ref(owner.0)), vec![XVal::Str(name.to_string()), XVal::Ref(owner.0)], None, ctx) {
                self.emit(crate::trace::EventKind::ScriptError { unit: "aux timer start handler".into(), error });
            }
        }
    }

    /// Each frame (`Game::expire_aux_timers`): the update handlers of every running,
    /// unpaused timer started with the Run OnTimerUpdate Event flag (256) get its name,
    /// its owner and the time since the last update, 1 for a timer counting frames
    /// (no flag 4) [documented: SetOnAuxTimerUpdateHandler; AuxTimerStart's flags].
    pub(crate) fn aux_timer_updates(&mut self) {
        // Handlers set or removed last frame take effect now [observed: showoff3
        // so3.aux.start.reg, a start in the step that set the handler runs nothing, a
        // restart one frame later runs it; so3.aux.start.remove, a restart in the step that
        // removed it still runs it].
        for (start, set, f, timer, owner) in std::mem::take(&mut self.showoff.pending_handlers) {
            let hs = if start { &mut self.showoff.timer_starts } else { &mut self.showoff.timer_updates };
            hs.retain(|(h, n, o)| !(*h == f && *n == timer && *o == owner));
            if set {
                hs.push((f, timer, owner));
            }
        }
        let now = self.time;
        let dt = now - self.showoff.last_update.unwrap_or(now);
        self.showoff.last_update = Some(now);
        if self.showoff.timer_updates.is_empty() {
            return;
        }
        let mut due: Vec<(FormId, String, f64)> = Vec::new();
        for (k, t) in &self.jip2.timers {
            let flags = self.showoff.countdowns.get(k).map_or(7, |c| c.1);
            if t.2 || flags & 256 == 0 {
                continue;
            }
            due.push((k.0, k.1.clone(), if flags & 4 == 0 { 1.0 } else { dt }));
        }
        due.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        for (owner, name, d) in due {
            let hs: Vec<XVal> = self.showoff.timer_updates.iter().filter(|(_, n, o)| *n == name && *o == owner).map(|(f, _, _)| f.clone()).collect();
            for f in hs {
                let ctx = RunCtx { self_ref: Some(owner), ..RunCtx::default() };
                let args = vec![XVal::Str(name.clone()), XVal::Ref(owner.0), XVal::Num(d)];
                if let Err(error) = self.run_function(&f, Some(fnv_script::vm::Value::Ref(owner.0)), args, None, ctx) {
                    self.emit(crate::trace::EventKind::ScriptError { unit: "aux timer update handler".into(), error });
                }
            }
        }
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- equipped items ----
        // `[mask]`: the bases, or inventory references to them [documented:
        // GetEquippedItems; GetEquippedItemRefs].
        "getequippeditems" | "getequippeditemrefs" => {
            let Some(a) = subject else { return Some(Ok(list(g, std::iter::empty()))) };
            let items = equipped(g, a, num(args, 0, 0.0) as i64 as u32);
            let vals: Vec<XVal> = items.into_iter().map(|(i, _)| if name == "getequippeditems" { XVal::Ref(i.0) } else { XVal::Ref(g.temp_ref(a, i).0) }).collect();
            list(g, vals.into_iter())
        }
        // The slots taken, non-playable items too [documented: GetEquippedItemsAsBitMask,
        // alias GetOccupiedEquipSlots].
        "getequippeditemsasbitmask" | "getoccupiedequipslots" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let items = equipped(g, a, SLOT_BITS);
            XVal::Num(f64::from(items.iter().fold(0u32, |m, (i, _)| m | slots_of(g, *i))))
        }
        // `[healthPercThreshold] [mask]`: weapons and armour at or below the health
        // percentage, 0 by default [documented: GetNumBrokenEquippedItems, alias
        // GetNumBrokenEq]. A unit with no condition kept is at full health.
        "getnumbrokenequippeditems" | "getnumbrokeneq" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let threshold = num(args, 0, 0.0);
            let items = equipped(g, a, num(args, 1, 0.0) as i64 as u32);
            let n = items.iter().filter(|(i, e)| g.data.sig(*i).is_some_and(|s| &s == b"WEAP" || &s == b"ARMO") && f64::from(e.condition.unwrap_or(1.0)) * 100.0 <= threshold).count();
            XVal::Num(n as f64)
        }
        // `[minWeight] [mask]`: the base weights, or the weights the carrier's perks and
        // the weapon's Decrease Weight mods leave [documented: GetBaseEquippedWeight,
        // "ignores weight modification effects"; GetCalculatedEquippedWeight: Modify Light
        // Items, Decrease Weight, Adjust Heavy Weapon Weight]. An item counts when that
        // weight is at least minWeight [hypothesis: the page does not say which weight
        // the minimum is held against in the calculated case].
        "getbaseequippedweight" | "getcalculatedequippedweight" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let min = num(args, 0, 0.0);
            let items = equipped(g, a, num(args, 1, 0.0) as i64 as u32);
            let rules = g.world.weight_rules(a);
            let forms = g.world.forms;
            let total: f64 = items
                .iter()
                .filter_map(|(i, e)| {
                    let it = forms.item(*i)?;
                    let w = if name == "getbaseequippedweight" { it.weight? } else { rules.unit(it.weight_with_mods(e.mods)?, it.kind) };
                    (f64::from(w) >= min).then_some(f64::from(w))
                })
                .sum();
            XVal::Num(total)
        }
        // Carry Weight with the Get Max Carry Weight perks [documented:
        // GetCalculatedMaxCarryWeight], as `functions::actor::sync_carry` gives the
        // inventory's weight line.
        "getcalculatedmaxcarryweight" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let cw = crate::functions::actor::actor_value(g, a, crate::actor_values::CARRY_WEIGHT).unwrap_or(0.0);
            XVal::Num(g.entry_point(a, crate::perks::GET_MAX_CARRY_WEIGHT, cw).0)
        }
        // `[mask] [noEquip] [hideMessage] [triggerOnUnequip]`: as UnequipItem on each
        // [documented: UnequipItems]. The OnUnequip blocks run whatever the last argument
        // says [not built: skipping them].
        "unequipitems" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let items = equipped(g, a, num(args, 0, 0.0) as i64 as u32);
            let (no_equip, hide) = (num(args, 1, 0.0) as i64, num(args, 2, 0.0) as i64);
            for (i, _) in items {
                let words = vec![format!("{:08X}", i.0), format!("{no_equip}"), format!("{hide}")];
                if let Err(e) = crate::functions::console::call_words(g, Some(a), "UnequipItem", &words, "UnequipItems") {
                    return Some(Err(e));
                }
            }
            zero
        }
        // The weapon as an inventory reference, 0 with none [documented:
        // GetEquippedWeaponRef].
        "getequippedweaponref" => {
            let Some(a) = subject else { return Some(Ok(XVal::Ref(0))) };
            match crate::functions::items::equipped_weapon(g, a) {
                Some(w) => XVal::Ref(g.temp_ref(a, w).0),
                None => XVal::Ref(0),
            }
        }
        // `itemOrFormList`: the item, or the first of the list's items, equipped, as an
        // inventory reference; 0 when none is [documented: GetEquippedItemRefForItem].
        "getequippeditemrefforitem" => {
            let (Some(a), Some(f)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Ref(0))) };
            let candidates = if g.data.sig(f) == Some(*b"FLST") { crate::functions::list_members(g, f) } else { vec![f] };
            let worn: Vec<FormId> = g.world.get(a).ok().and_then(|s| s.inventory.as_ref()).map(|inv| inv.equipped()).unwrap_or_default();
            match candidates.into_iter().find(|c| worn.contains(c)) {
                Some(i) => XVal::Ref(g.temp_ref(a, i).0),
                None => XVal::Ref(0),
            }
        }
        // ---- the seeded randomizer, one seed per mod ----
        // `[seed]`: 0 or none the default seed; the sequence starts again [documented:
        // SetRandomizerSeed, alias SetSeed].
        "setrandomizerseed" | "setseed" => {
            let s = uint(args, 0);
            let s = if s == 0 { DEFAULT_SEED } else { s };
            let m = g.other.plugin.to_ascii_lowercase();
            g.showoff.seeds.insert(m, Seeded::new(s));
            zero
        }
        // `[form]`: the form's FormID, else the default [documented: SetSeedUsingForm,
        // alias SetFormSeed]; the calling reference is not used [observed: showoff3
        // so3.seed.form.ref, player.SetSeedUsingForm reads 5489].
        "setseedusingform" | "setformseed" => {
            let s = form(args, 0).map_or(DEFAULT_SEED, |f| f.0);
            let m = g.other.plugin.to_ascii_lowercase();
            g.showoff.seeds.insert(m, Seeded::new(s));
            zero
        }
        // -1 with no seed set [documented: GetRandomizerSeed, alias GetSeed, "If no seed
        // is set, returns -1"].
        "getrandomizerseed" | "getseed" => {
            let m = g.other.plugin.to_ascii_lowercase();
            XVal::Num(g.showoff.seeds.get(&m).map_or(-1.0, |s| f64::from(s.seed)))
        }
        // `min max`, both included; -1 when min is above max [documented: RandSeeded,
        // alias GenerateSeededRandomNumber]; the draw scaled onto the range
        // ([`DEFAULT_SEED`]).
        "randseeded" | "generateseededrandomnumber" | "getrandompercentseeded" => {
            let (lo, hi) = if name == "getrandompercentseeded" { (1, 100) } else { (uint(args, 0), uint(args, 1)) };
            if lo > hi {
                return Some(Ok(XVal::Num(-1.0)));
            }
            let span = u64::from(hi - lo) + 1;
            // A draw whose product's low 32 bits fall under 2^32 mod span is thrown away and
            // the next one taken, so every value is as likely [observed: showoff3
            // so3.seed.range.wide, 0..=1000000000 skips MT19937's third output for 5489 and
            // gives the fourth's scaled value].
            let reject = (1u64 << 32) % span;
            let rng = &mut g.mod_seed().rng;
            let m = loop {
                let m = u64::from(rng.next_u32()) * span;
                if m & 0xFFFF_FFFF >= reject {
                    break m;
                }
            };
            XVal::Num(f64::from(lo) + (m >> 32) as f64)
        }
        // `n`: n draws thrown away; 0 when the mod has no seed [documented: AdvanceSeed].
        "advanceseed" => {
            let m = g.other.plugin.to_ascii_lowercase();
            // At most ADVANCE_SEED_MAX draws a call [design]: the page gives no limit, and a
            // huge or garbage count (1e12) would hold the frame for hours.
            let n = (num(args, 0, 0.0).max(0.0) as u64).min(ADVANCE_SEED_MAX);
            match g.showoff.seeds.get_mut(&m) {
                Some(s) => {
                    for _ in 0..n {
                        s.rng.next_u32();
                    }
                    XVal::Num(1.0)
                }
                None => zero,
            }
        }
        // ---- small getters ----
        // `input code mode` [documented: ApplyEasingAlt, alias EaseAlt; -1 for an unknown
        // curve]; -1 for a mode other than 0-2, and an input outside 0-1 eased as it is
        // [observed: showoff3 so3.ease.badmode, so3.ease.outside].
        "applyeasingalt" | "easealt" => {
            let code = num(args, 1, -1.0);
            let mode = num(args, 2, 0.0);
            match usize::try_from(code as i64).ok().and_then(|c| EASINGS.get(c)) {
                Some(e) if code.fract() == 0.0 && (0.0..=2.0).contains(&mode) => XVal::Num(crate::nvse_plugins::ease_unclamped(num(args, 0, 0.0), e, mode as i64)),
                _ => XVal::Num(-1.0),
            }
        }
        // X, Y, Z [documented: GetPosArray].
        "getposarray" => {
            let p = subject.and_then(|r| place(g, r)).map_or([0.0; 3], |p| p.1);
            list(g, p.iter().map(|&c| XVal::Num(f64::from(c))))
        }
        // NVForever runs every script on its main thread [documented: IsOutsideMainThread,
        // true only off the main thread].
        "isoutsidemainthread" => zero,
        // 1, and a console line naming the reference [documented: ConditionPrint; the line
        // is NVForever's own words].
        "conditionprint" => {
            let line = match subject {
                Some(r) => format!("ConditionPrint >> reference {:08X} ({})", r.0, g.data.name(r)),
                None => "ConditionPrint >> no reference".to_string(),
            };
            g.console_out.push(line);
            XVal::Num(1.0)
        }
        // IsAnimPlayingEx's first two arguments [documented: IsAnimPlayingExCond].
        "isanimplayingexcond" => return crate::nvse::call(g, ctx, "isanimplayingex", subject, args),
        // "functions equivalently to IsInAir" [documented: IsJumping, marked broken].
        "isjumping" => return crate::nvse::call(g, ctx, "isinair", subject, &[]),
        // `maxDistance [flags]`: the actors within the distance; flag 1 no dead ones, 2 no
        // invisible ones (Invisibility or Chameleon above 0), 4 only ones in combat the
        // calling reference detects [documented: GetNumActorsInRangeFromRef]. The calling
        // reference does not count itself [hypothesis].
        "getnumactorsinrangefromref" => {
            let Some(me) = subject else { return Some(Ok(zero)) };
            let max = num(args, 0, 0.0);
            let flags = num(args, 1, 0.0) as i64;
            let mut actors: Vec<FormId> = g.package_actors().into_iter().collect();
            actors.push(FormId::PLAYER_REF);
            actors.sort();
            actors.dedup();
            let codes = [crate::functions::actor::av_code(g, "Invisibility").ok(), crate::functions::actor::av_code(g, "Chameleon").ok()];
            let mut n = 0;
            for a in actors {
                if a == me || distance(g, me, a).is_none_or(|d| d > max) || !g.world.is_enabled(a).unwrap_or(true) {
                    continue;
                }
                if flags & 1 != 0 && g.extra.get(&a).is_some_and(|e| e.dead) {
                    continue;
                }
                if flags & 2 != 0 && codes.iter().flatten().any(|&c| crate::functions::actor::actor_value(g, a, c).unwrap_or(0.0) > 0.0) {
                    continue;
                }
                if flags & 4 != 0 && !(g.combat_target_of(a).is_some() && g.detection.get(me, a).is_some_and(|d| d.band == crate::detection::Band::Detected)) {
                    continue;
                }
                n += 1;
            }
            XVal::Num(f64::from(n))
        }
        // ---- the player ----
        // [documented: SetIsPCAMurderer; IsPCAMurderer reads the flag].
        "setispcamurderer" => {
            g.player.murderer = num(args, 0, 0.0) != 0.0;
            zero
        }
        // EnableFastTravel's "can wait" flag [documented: Get/SetPCHasSleepWaitOverride,
        // "EnableFastTravel modifies the same canSleepWait override flag"; the wait menu
        // refuses while it is on].
        "getpchassleepwaitoverride" => yes(g.player.fast_travel.wait_disabled),
        "setpchassleepwaitoverride" => {
            g.player.fast_travel.wait_disabled = num(args, 0, 0.0) != 0.0;
            zero
        }
        // `challenge`: its progress; a challenge done that does not recur reads its
        // threshold, a recurring one counts again from 0 [documented: GetChallengeProgress,
        // notes], as `Game::challenge_step` keeps it.
        "getchallengeprogress" => {
            let Some(c) = form(args, 0).filter(|c| g.data.challenge_defs.contains_key(c)) else { return Some(Ok(zero)) };
            XVal::Num(f64::from(g.player.challenges.get(&c).map_or(0, |s| s.progress)))
        }
        // `[levelOverride]`: the skill points of the next level up, or of this one while
        // the level-up menu is open; the perks' Adjust Gained Skill Points counted
        // [documented: GetCalculatedSkillPoints, alias
        // GetCalculatedSkillPointsEarnedPerLevel; `crate::levelup::points_for`].
        "getcalculatedskillpoints" | "getcalculatedskillpointsearnedperlevel" | "getcalculatedperkpoints" | "getcalculatedperkpointsearnedperlevel" => {
            let over = num(args, 0, 0.0) as i32;
            let level = if over != 0 { over } else { g.player_level() as i32 };
            let level = if g.menu_open(crate::game::menus::LEVEL_UP) { level } else { level + 1 };
            // Perk points: 1 at a level iLevelsPerPerk divides, else 0 [documented:
            // GetCalculatedPerkPoints, "this function can only return 1 or 0"].
            XVal::Num(if name.starts_with("getcalculatedperk") { f64::from(u8::from(crate::levelup::perk_due(g, level))) } else { f64::from(crate::levelup::points_for(g, level)) })
        }
        // ---- factions ----
        // `f1 f2 [flag12 flag21]`: SetAlly and SetEnemy for the session, not saved
        // [documented: SetAllyTemp, alias SetAllyShowOff, flags 0 ally, 1 friend;
        // SetEnemyTemp, alias SetEnemyShowOff, 0 enemy, 1 neutral]. They win over the
        // saved reactions until SetAlly or SetEnemy is called on the pair
        // (`functions::actor::reaction`).
        "setallytemp" | "setallyshowoff" | "setenemytemp" | "setenemyshowoff" => {
            let (Some(f1), Some(f2)) = (form(args, 0), form(args, 1)) else { return Some(Ok(zero)) };
            let ally = name.starts_with("setally");
            let v = |x: f64| match (ally, x != 0.0) {
                (true, true) => 3,
                (true, false) => 2,
                (false, true) => 0,
                (false, false) => 1,
            };
            let (r12, r21) = (v(num(args, 2, 0.0)), v(num(args, 3, 0.0)));
            g.showoff.temp_reactions.insert((f1, f2), r12);
            g.showoff.temp_reactions.insert((f2, f1), r21);
            crate::functions::actor::forget_relations(g);
            zero
        }
        // ---- aux timers ----
        // `timerName [owner]`: the time it was started with, -1 when it is not running
        // [documented: AuxTimerTimeToCountdown]. The owner argument is a base form: a
        // reference given there stands for its base [observed: showoff3 so3.aux.countdown,
        // the runway piece's running timer named by argument reads -1, called on it 30].
        "auxtimertimetocountdown" => {
            let owner = form(args, 1).map(|f| crate::functions::base_of(g, f).unwrap_or(f)).or(subject).unwrap_or(FormId::PLAYER_REF);
            let key = (owner, text(args, 0).to_ascii_lowercase());
            XVal::Num(if g.jip2.timers.contains_key(&key) { g.showoff.countdowns.get(&key).map_or(-1.0, |c| c.0) } else { -1.0 })
        }
        // `1/0 handler timerName [owner]`, the owner the player when none [documented:
        // SetOnAuxTimerStartHandler, alias SetOnTimerStartHandler;
        // SetOnAuxTimerUpdateHandler, alias SetOnTimerUpdateHandler].
        "setonauxtimerstarthandler" | "setontimerstarthandler" | "setonauxtimerupdatehandler" | "setontimerupdatehandler" => {
            let set = num(args, 0, 1.0) != 0.0;
            let Some(f) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let timer = text(args, 2).to_ascii_lowercase();
            let owner = form(args, 3).or_else(|| subject.filter(|_| args.len() < 4)).unwrap_or(FormId::PLAYER_REF);
            // Applied at the next frame (`Game::aux_timer_updates`).
            g.showoff.pending_handlers.push((name.contains("start"), set, f, timer, owner));
            XVal::Num(1.0)
        }
        // ---- a reference's radiation ----
        // XRAD, the rads a second a marker gives [documented: GetRadiationExtraData, 0
        // without; SetRadiationExtraData, "If it has no Radiation ExtraData, it is added";
        // the player's exposure reads the change, `Game::tick_radiation`]. The change is
        // savebaked in the original; here it lasts for the session [not built: saving].
        "getradiationextradata" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            XVal::Num(g.showoff.radiation.get(&r).copied().or_else(|| g.world.placed.get(r).and_then(|p| p.radiation)).map_or(0.0, f64::from))
        }
        "setradiationextradata" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            g.showoff.radiation.insert(r, num(args, 0, 0.0) as f32);
            XVal::Num(1.0)
        }
        // `setting by`: GetNumericINISetting plus the amount, set back; 0 for no such
        // setting [documented: ModNumericINISetting].
        "modnumericinisetting" => {
            let setting = text(args, 0);
            let now = match crate::nvse::call(g, ctx, "getnumericinisetting", subject, &[XVal::Str(setting.clone())]) {
                Some(Ok(XVal::Num(x))) => x,
                _ => return Some(Ok(zero)),
            };
            match crate::nvse::call(g, ctx, "setnumericinisetting", subject, &[XVal::Str(setting), XVal::Num(now + num(args, 1, 0.0))]) {
                Some(Ok(v)) => v,
                _ => zero,
            }
        }
        // `info`: the response texts in order [documented: TopicInfoGetResponseStrings]; no
        // array for a form that is not a line [observed: showoff3 so3.responses.stimpak,
        // ar_Size -1].
        "topicinfogetresponsestrings" => {
            let Some(texts) = form(args, 0).and_then(|i| g.dialogue_view().info(&i).map(|d| d.responses.iter().map(|r| XVal::Str(r.text.clone())).collect::<Vec<XVal>>())) else {
                return Some(Ok(XVal::Array(0)));
            };
            list(g, texts.into_iter())
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// The most draws one `AdvanceSeed` throws away [design]: about 10 ms of MT19937, so a
/// script's huge or garbage count cannot freeze the game.
const ADVANCE_SEED_MAX: u64 = 1_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    /// [rand.predef]'s check: with seed 5489 the 10 000th output is 4123659995.
    #[test]
    fn mt19937_matches_the_reference_sequence() {
        let mut m = Mt19937::new(DEFAULT_SEED);
        let first = m.next_u32();
        assert_eq!(first, 3_499_211_612);
        let mut last = first;
        for _ in 1..10_000 {
            last = m.next_u32();
        }
        assert_eq!(last, 4_123_659_995);
    }
}
