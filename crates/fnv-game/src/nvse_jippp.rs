//! JIP PP LN's functions (#1019): the runtime node lists (AttachFormModel,
//! CopyRuntimeNodes, ClearRuntimeNodes, HasRuntimeNode), the active effects as arrays,
//! a weapon's wear per shot, a control's code, ClampToGround and RefreshInventoryWeight
//! [documented: each page in the wiki export; docs/spec-notes/ext-jippp.md]. JIP PP LN
//! is not installed in the probe game (its jip_nvse.dll is JIP LN's), so nothing here is
//! observed.

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

/// One entry of a form's runtime node list: an InsertNode or AttachModel call (or an
/// AttachFormModel / CopyRuntimeNodes copy of one) [documented: HasRuntimeNode, "the
/// runtime data created by AttachModel and InsertNode"].
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeNode {
    /// AttachModel's (a model under a node) rather than InsertNode's (a new node).
    pub attached: bool,
    /// The call's `"Target|Name"` (or `"Target|model.nif"`) as given.
    pub spec: String,
    /// The plugin whose script made it (ClearRuntimeNodes clears its caller's only).
    pub plugin: String,
}

/// JIP PP LN's session state.
#[derive(Debug, Default)]
pub struct JipppState {
    /// The runtime node lists, by base form or reference.
    pub nodes: HashMap<FormId, Vec<RuntimeNode>>,
    /// ClampToGround calls, for the engine (which knows the ground) to take.
    pub clamp: Vec<FormId>,
}

impl Game<'_> {
    /// The engine takes the references ClampToGround asked to drop to the floor.
    pub fn take_clamp_requests(&mut self) -> Vec<FormId> {
        std::mem::take(&mut self.jippp.clamp)
    }

    /// A reference's InsertNode entries in each of its drawn models that lacks them: a
    /// model drawn after the call (the player's before its 3D is built, as a Script Runner
    /// file at a load) or drawn anew (the player's body rebuilt) [documented: the
    /// addOrRemove table as ext-jippp.md §1 gives it, mode 3 also at once on a drawn
    /// reference; hypothesis: the list's nodes are built into the 3D whenever it is
    /// built]. Each frame (`Game::tick`); base forms' lists are not replayed
    /// (ext-jippp.md §1).
    pub(crate) fn insert_runtime_nodes(&mut self) {
        if self.jippp.nodes.is_empty() {
            return;
        }
        let wanted: Vec<(FormId, String)> = self.jippp.nodes.iter().flat_map(|(f, l)| l.iter().filter(|n| !n.attached).map(|n| (*f, n.spec.clone()))).collect();
        for (f, spec) in wanted {
            let Some((target, what)) = spec.split_once('|') else { continue };
            let (as_parent, name) = match what.strip_prefix('^') {
                Some(n) => (true, n),
                None => (false, what),
            };
            self.scene.insert_node(f, target, name, as_parent);
        }
    }
}

/// An InsertNode or AttachModel call into the form's runtime node list: modes 1 and 3
/// add, 0 and 2 remove [documented: AttachFormModel's addOrRemove table, which the pages
/// of the three share]. Called from `crate::nif_blocks`.
pub(crate) fn record(g: &mut Game, f: FormId, attached: bool, spec: &str, mode: f64) {
    if spec.is_empty() {
        return;
    }
    let list = g.jippp.nodes.entry(f).or_default();
    let add = mode as i64 & 1 == 1;
    list.retain(|n| !(n.attached == attached && n.spec == spec));
    if add {
        list.push(RuntimeNode { attached, spec: spec.to_string(), plugin: g.other.plugin.clone() });
    }
}

/// The node a spec adds: the part after `|`, without InsertNode's `^` [documented:
/// InsertNode].
fn added_name(spec: &str) -> Option<&str> {
    spec.split_once('|').map(|(_, n)| n.trim_start_matches('^'))
}

/// An active effect's Form filter class [documented: GetActiveEffectsInfo's Effect
/// Filter Types]: 1 object effects (ENCH), 2 actor effects (SPEL), 3 ingestibles (ALCH).
fn effect_class(g: &Game, source: FormId) -> i64 {
    match g.data.sigs.get(&source).map(|s| &s[..]) {
        Some(b"ENCH") => 1,
        Some(b"SPEL") => 2,
        Some(b"ALCH") => 3,
        _ => 0,
    }
}

/// Health points a weapon loses for one shot with that ammunition: the record's Damage
/// to Weapon Mult when its override flag is set, else fDamageToWeaponValue, then the
/// ammunition's Weapon Condition Mod effects in order [documented:
/// GetCalculatedWeaponDegradation, "If a weapon has a 'damage to weapon' override, this
/// will also be factored"; the rule is fnv_combat::condition::weapon_wear's,
/// docs/spec-notes/combat-rules.md §17].
fn weapon_degradation(g: &Game, weapon: FormId, ammo: Option<FormId>) -> f64 {
    let n = |f: FormId, sub: &str, path: &str| match crate::nvse::record_value(g, f, sub, path) {
        Some(XVal::Num(x)) => Some(x),
        _ => None,
    };
    let flags2 = n(weapon, "DNAM", "flags2").unwrap_or(0.0) as u64;
    let mut v = if flags2 & (1 << 7) != 0 {
        n(weapon, "DNAM", "override_damage_to_weapon_mult").unwrap_or(0.0)
    } else {
        g.world.setting_f32("fDamageToWeaponValue").map_or(0.0, f64::from)
    };
    // AddAmmoEffect's, RemoveAmmoEffect's, and live AMEF trait changes first
    // (`crate::nvse_jip7`), type 4 = Weapon Condition Mod.
    if let Some(a) = ammo {
        v = g.apply_ammo_effects_value(a, 4, v);
    }
    crate::nvse::record_float(v.max(0.0))
}

/// An extender call made by a script of `plugin` (lower case, e.g. "mymod.esp"): the
/// runtime node lists keep and clear entries by the calling plugin, which a query or the
/// console does not have. For tests and for callers that know their plugin.
pub fn call_as(g: &mut Game, plugin: &str, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    g.other.plugin = plugin.to_ascii_lowercase();
    let r = crate::nvse::call(g, &RunCtx::default(), &name.to_ascii_lowercase(), subject, args);
    g.other.plugin.clear();
    r
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // `target toAttach mode "node|suffix"`: the form's model (with its own runtime
        // nodes) under the target's node, its root without one; modes 1 and 3 attach, 0
        // and 2 remove, 2 and 3 at once on a drawn reference [documented: AttachFormModel].
        // 1 on success: the form has a model (attaching) or was attached (removing). The
        // suffix is kept in the entry [hypothesis: the engine does not rename the attached
        // blocks yet, follow-up].
        "attachformmodel" => {
            let (Some(target), Some(src)) = (form(args, 0), form(args, 1)) else { return Some(Ok(zero)) };
            let mode = num(args, 2, 0.0) as i64;
            let spec = text(args, 3);
            let (node, suffix) = match spec.split_once('|') {
                Some((n, s)) => (n.to_string(), s.trim_matches('*').chars().take(15).collect::<String>()),
                None => (spec.clone(), String::new()),
            };
            let Some(model) = crate::nif_blocks::model_of(g, src) else { return Some(Ok(zero)) };
            let entry = format!("{node}|{model}");
            let list = g.jippp.nodes.entry(target).or_default();
            list.retain(|n| !(n.attached && n.spec == entry));
            // A remove succeeds for a form with a model whether or not the entry was
            // there [observed: jipppln, a second remove gives 1 again].
            if mode & 1 == 0 {
                return Some(Ok(XVal::Num(1.0)));
            }
            let plugin = g.other.plugin.clone();
            list.push(RuntimeNode { attached: true, spec: entry, plugin: plugin.clone() });
            let extra: Vec<RuntimeNode> = g.jippp.nodes.get(&src).cloned().unwrap_or_default();
            let list = g.jippp.nodes.entry(target).or_default();
            for mut n in extra {
                if !suffix.is_empty() {
                    n.spec = format!("{}*{suffix}*", n.spec);
                }
                n.plugin.clone_from(&plugin);
                if !list.contains(&n) {
                    list.push(n);
                }
            }
            if mode == 3 && crate::functions::base_of(g, target).is_some() {
                let at = (!node.is_empty()).then_some(node.as_str());
                g.attach_model_now(target, at, &model);
            }
            XVal::Num(1.0)
        }
        // `to from [mode]`: 0 replaces the target's list with a copy of the source's, 1
        // appends it; 1 on success [documented: CopyRuntimeNodes; hypothesis: success is
        // both forms being valid, an empty source too].
        "copyruntimenodes" => {
            let (Some(to), Some(from)) = (form(args, 0), form(args, 1)) else { return Some(Ok(zero)) };
            // A script of no plugin (the console, a runtime-compiled script) copies
            // nothing: 0 [observed: jipppln, from the probe's own runtime-compiled script;
            // hypothesis: the reason, as ClearRuntimeNodes's].
            if g.other.plugin.is_empty() {
                return Some(Ok(zero));
            }
            if !g.data.sigs.contains_key(&to) && crate::functions::base_of(g, to).is_none() {
                return Some(Ok(zero));
            }
            let src = g.jippp.nodes.get(&from).cloned().unwrap_or_default();
            let list = g.jippp.nodes.entry(to).or_default();
            if num(args, 2, 0.0) as i64 != 1 {
                list.clear();
            }
            list.extend(src);
            XVal::Num(1.0)
        }
        // `object`: the calling plugin's entries dropped, other plugins' kept; the number
        // dropped [documented: ClearRuntimeNodes, "if ClearRuntimeNodes … > 0"]. On a
        // drawn reference the inserted nodes go at once (see docs/spec-notes/ext-jippp.md §1).
        "clearruntimenodes" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let plugin = g.other.plugin.clone();
            // A script of no plugin owns no entries: 0, nothing cleared [observed:
            // jipppln, after the probe's own InsertNode; hypothesis: the entries count as
            // no mod's].
            if plugin.is_empty() {
                return Some(Ok(zero));
            }
            let list = g.jippp.nodes.entry(f).or_default();
            let gone: Vec<RuntimeNode> = list.iter().filter(|n| n.plugin == plugin).cloned().collect();
            list.retain(|n| n.plugin != plugin);
            for n in gone.iter().filter(|n| !n.attached) {
                if let Some(nm) = added_name(&n.spec) {
                    if crate::functions::base_of(g, f).is_some() {
                        g.remove_node_now(f, nm);
                    }
                }
            }
            XVal::Num(gone.len() as f64)
        }
        // `object query [mode]`: 2 an AttachModel entry, 1 an InsertNode entry, of exactly
        // that spec; 0 any entry whose target path (before `|`) holds the text [documented:
        // HasRuntimeNode's modes, "nodes that contain this path"; observed: jipppln, the
        // new node's name and "Bip01|" read 0; hypothesis: mode 0 by default, exact case].
        "hasruntimenode" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let q = text(args, 1);
            let mode = num(args, 2, 0.0) as i64;
            let list = g.jippp.nodes.get(&f).map(Vec::as_slice).unwrap_or(&[]);
            let found = match mode {
                2 => list.iter().any(|n| n.attached && n.spec == q),
                1 => list.iter().any(|n| !n.attached && n.spec == q),
                _ => !q.is_empty() && list.iter().any(|n| n.spec.split('|').next().unwrap_or("").contains(&q)),
            };
            XVal::Num(f64::from(u8::from(found)))
        }
        // `[filter] [temp]`: each active effect on the calling actor as a string map
        // [documented: GetActiveEffectsInfo's entries and filters]. Flags read 0 and a
        // permanent effect's TimeLeft and Duration 0 (see docs/spec-notes/ext-jippp.md §2).
        "getactiveeffectsinfo" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let (class, by_form) = match args.first() {
                Some(XVal::Ref(r)) if *r != 0 => (0, Some(FormId(*r))),
                Some(XVal::Num(x)) if *x > 3.0 => (0, Some(FormId(*x as u32))),
                Some(XVal::Num(x)) => (*x as i64, None),
                _ => (0, None),
            };
            let temp = num(args, 1, 2.0) as i64;
            let rows: Vec<crate::game::ActiveEffect> = g
                .effects
                .iter()
                .filter(|e| e.target == a)
                .filter(|e| class == 0 || effect_class(g, e.source) == class)
                .filter(|e| by_form.is_none_or(|f| e.effect == f || e.source == f))
                .filter(|e| {
                    let permanent = e.permanent || e.worn.is_some();
                    match temp {
                        0 => permanent,
                        1 => !permanent,
                        _ => true,
                    }
                })
                .cloned()
                .collect();
            let mut out = Vec::new();
            for (i, e) in rows.iter().enumerate() {
                let permanent = e.permanent || e.worn.is_some();
                let (left, dur) = if permanent { (0.0, 0.0) } else { ((e.duration - e.elapsed).max(0.0), e.duration) };
                let m = vec![
                    (XKey::Str("Form".into()), XVal::Ref(e.source.0)),
                    (XKey::Str("BaseEffect".into()), XVal::Ref(e.effect.0)),
                    (XKey::Str("Caster".into()), XVal::Ref(e.caster.map_or(0, |c| c.0))),
                    (XKey::Str("MetaData".into()), XVal::Num(f64::from(e.id))),
                    (XKey::Str("Magnitude".into()), XVal::Num(f64::from(e.magnitude))),
                    (XKey::Str("TimeLeft".into()), XVal::Num(left)),
                    (XKey::Str("Duration".into()), XVal::Num(dur)),
                    (XKey::Str("Flags".into()), XVal::Num(0.0)),
                ];
                out.push((XKey::Num(i as f64), XVal::Array(g.ext.new_array(ArrayKind::StringMap, m))));
            }
            // No effect to list gives no array [observed: jipppln, ar_Size −1].
            if out.is_empty() {
                return Some(Ok(XVal::Array(0)));
            }
            XVal::Array(g.ext.new_array(ArrayKind::List, out))
        }
        // `array`: GetActiveEffectsInfo's maps written back by MetaData: Magnitude,
        // TimeLeft (at most the duration) and Duration [documented: SetActiveEffectsInfo;
        // the read-only entries are not read]. A per-second change scales with the
        // magnitude (see docs/spec-notes/ext-jippp.md §2; follow-up).
        "setactiveeffectsinfo" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let Some(XVal::Array(id)) = args.first() else { return Some(Ok(zero)) };
            let rows: Vec<XVal> = g.ext.array(*id).map(|ar| ar.entries.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
            for row in rows {
                let XVal::Array(rid) = row else { continue };
                let Some(m) = g.ext.array(rid) else { continue };
                let get = |k: &str| match m.get(&XKey::Str(k.into())) {
                    Some(XVal::Num(x)) => Some(*x),
                    _ => None,
                };
                let (Some(meta), mag, left, dur) = (get("MetaData"), get("Magnitude"), get("TimeLeft"), get("Duration")) else { continue };
                let Some(e) = g.effects.iter_mut().find(|e| e.target == a && f64::from(e.id) == meta) else { continue };
                if let Some(mag) = mag {
                    let new = mag.round().max(0.0) as u32;
                    if let Some(c) = e.change.as_mut().filter(|c| c.kind == crate::aid::ChangeKind::PerSecond) {
                        if e.magnitude > 0 {
                            c.amount *= f64::from(new) / f64::from(e.magnitude);
                        }
                    }
                    e.magnitude = new;
                }
                if !e.permanent {
                    if let Some(d) = dur {
                        e.duration = d.max(0.0);
                    }
                    if let Some(l) = left {
                        e.elapsed = e.duration - l.clamp(0.0, e.duration);
                    }
                }
            }
            zero
        }
        // `[weapon] [ammo]` on an actor: the weapon given, else the one it holds; the
        // ammunition given, else the one it uses, else the weapon's own [documented:
        // GetCalculatedWeaponDegradation's examples].
        "getcalculatedweapondegradation" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let weapon = form(args, 0).map(|w| crate::functions::base_of(g, w).unwrap_or(w)).filter(|w| g.data.sigs.get(w).is_some_and(|s| s == b"WEAP"));
            let weapon = weapon.or_else(|| crate::functions::items::equipped_weapon(g, a));
            let Some(w) = weapon else { return Some(Ok(zero)) };
            let ammo = form(args, 1).or_else(|| if form(args, 0).is_none() { g.ammo_in_use(a) } else { None }).or_else(|| match crate::nvse::record_value(g, w, "NAM0", "") {
                Some(XVal::Ref(r)) if r != 0 => Some(FormId(r)),
                _ => None,
            });
            XVal::Num(weapon_degradation(g, w, ammo))
        }
        // `control type`: 0 its key, 1 its mouse button (as GetControl gives it, 256 + n)
        // [documented: GetControlCodeByID's Control Types; hypothesis: the mouse code's
        // form]; 2 and 3 (controller, joystick) unbound, NVForever binds no pad to the
        // controls [decision].
        "getcontrolcodebyid" => {
            let ty = num(args, 1, 0.0) as i64;
            if !(0..=1).contains(&ty) {
                return Some(Ok(XVal::Num(crate::script_input::UNBOUND)));
            }
            let r = crate::nvse::call(g, ctx, "getcontrol", subject, &[args.first().cloned().unwrap_or(XVal::Num(-1.0)), XVal::Num(ty as f64)]);
            // A control with no key reads 255 [observed: jipppln, Attack's key]; no mouse
            // button stays −1 [observed].
            if ty == 0 {
                if let Some(Ok(XVal::Num(x))) = r {
                    if x < 0.0 {
                        return Some(Ok(XVal::Num(255.0)));
                    }
                }
            }
            return r;
        }
        // The engine drops the reference to the floor beneath it [documented:
        // ClampToGround]: the game knows no ground, so the call waits for the engine
        // (`Game::take_clamp_requests`; follow-up: the engine does not take it yet).
        "clamptoground" => {
            // A base form as the calling object stops the script [observed: jipppln
            // clamp.base, "script call failed"]: worded as the VM's NoReference, which
            // stops non-strict runs too. A weapon is its own base (`base_of`), so the
            // base must differ from the form.
            let Some(r) = subject.filter(|&r| crate::functions::base_of(g, r).is_some_and(|b| b != r)) else {
                return Some(Err(fnv_script::vm::VmError::NoReference(name.to_string()).to_string()));
            };
            crate::nvse_vheyes::queue_once(&mut g.jippp.clamp, r);
            zero
        }
        // [decision: NVForever reckons the carried weight afresh at each read
        // (`World::carried_weight`), so there is nothing to refresh].
        "refreshinventoryweight" => zero,
        _ => return None,
    };
    Some(Ok(v))
}
