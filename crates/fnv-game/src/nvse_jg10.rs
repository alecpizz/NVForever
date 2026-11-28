//! JohnnyGuitar NVSE, batch 10 (#1007): navmesh obstacles, triangles and paths, weapon
//! poisons, casings and muzzle flash lights, the weapon scope's picture, FaceGen values,
//! VATS body part highlights, the land texture underfoot and landscape remaps, idles,
//! the barber menu with lists, the screen splatter with textures, and the crosshair
//! prompt's and repair menu's refreshes [documented: each page in the wiki export;
//! docs/spec-notes/ext-jg10.md]. What the engine does not read yet is kept here for it
//! (spec note §3).

use crate::game::Game;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// PathToPoint's or PathToRef's request: the actor walks there without its package
/// changing [documented].
#[derive(Debug, Clone, PartialEq)]
pub struct PathRequest {
    pub actor: FormId,
    /// The point (PathToPoint) or the reference (PathToRef).
    pub to: PathTarget,
    /// TargetRadius: how near is there; 0 when not given.
    pub radius: f32,
    /// PathToPoint's TargetAngle (degrees) or PathToRef's FaceTarget (1: face it).
    pub angle: Option<f32>,
    pub face: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathTarget {
    Point([f32; 3]),
    Ref(FormId),
}

/// EjectCasing's request: a casing from the reference's weapon, from a node (else
/// ShellCasingNode) and of a model (else the weapon's) [documented].
#[derive(Debug, Clone, PartialEq)]
pub struct CasingRequest {
    pub actor: FormId,
    pub node: Option<String>,
    pub model: Option<String>,
}

/// TriggerScreenSplatterEx's textures and fade, for the engine's splats [documented].
#[derive(Debug, Clone, PartialEq)]
pub struct SplatterTextures {
    pub alpha: String,
    pub color: String,
    pub flare: String,
    pub fade: bool,
}

/// The session's state of this batch (none of it is saved: DisableMuzzleFlashLights
/// "does not persist through saves", HighlightBodyPartAlt is "session-persistent"
/// [documented]; the rest is a session's too [hypothesis]).
#[derive(Debug, Default)]
pub struct Jg10State {
    /// References scripts added to the navmesh obstacles, and those they took off.
    pub nav_obstacles: BTreeSet<FormId>,
    /// PathToPoint and PathToRef requests the engine has not taken.
    pub paths: Vec<PathRequest>,
    /// Poisons put on weapons: by (container, weapon), the container an actor (its
    /// equipped weapon) or the inventory reference's.
    pub poisons: HashMap<(FormId, FormId), FormId>,
    /// EjectCasing requests the engine has not taken.
    pub casings: Vec<CasingRequest>,
    /// DisableMuzzleFlashLights' mode: 0 lights on, 1 off for all, 2 the player's off,
    /// 3 NPCs' off [documented].
    pub muzzle_lights_off: u8,
    /// SetWeaponScopeUIModel's picture (`None`: the weapon's own).
    pub scope_model: Option<String>,
    /// ToggleWeaponScopeUIModel's last call (`None`: never called).
    pub scope_visible: Option<bool>,
    /// The body part highlighted: the actor and the part (-1: everything but the weapon).
    pub highlight: Option<(FormId, i64)>,
    /// RemapLand's swaps: the target LAND takes the source worldspace's cell (x, y).
    pub land_remaps: BTreeMap<FormId, (FormId, i32, i32)>,
    /// ShowBarberMenuEx's flag and list, for the barber menu it opened.
    pub barber_filter: Option<(u8, FormId)>,
    /// TriggerScreenSplatterEx's textures for the splats of its call.
    pub splatter_textures: Option<SplatterTextures>,
    /// UpdateCrosshairPrompt and UpdateRepairMenu calls so far.
    pub prompt_refreshes: u64,
    pub repair_refreshes: u64,
    /// The last land texture found under each reference, kept for where none lies under
    /// it (GetLandTextureUnderFeet indoors).
    pub last_land_texture: HashMap<FormId, u32>,
}

/// The FaceGen value lists of FaceGenGet/SetNthProperty's `Property Type`: 0 the
/// symmetric geometry (FGGS, 50), 1 the asymmetric geometry (FGGA, 30), 2 the symmetric
/// texture (FGTS, 50) [hypothesis: the page leaves the types undocumented; the order is
/// the records' (xEdit wbFaceGen); spec note §1].
const FACEGEN: [(&str, usize); 3] = [("FGGS", 50), ("FGGA", 30), ("FGTS", 50)];

/// How far up a GetLandTextureUnderFeet cast starts and how far down it looks, game
/// units [hypothesis: the feet stand on the ground; a cast from a little above reaches it].
const UNDERFOOT_UP: f32 = 64.0;
const UNDERFOOT_DOWN: f32 = 100_000.0;

impl Game<'_> {
    /// The poison on an actor's equipped weapon (GetEquippedWeaponPoison).
    pub fn weapon_poison(&mut self, actor: FormId) -> Option<FormId> {
        let w = crate::functions::items::equipped_weapon(self, actor)?;
        // The player's own poison on the weapon in hand (`crate::weapon_poison`) when no
        // script put one there.
        let own = (actor == FormId::PLAYER_REF).then(|| self.poison_on(w).map(|(p, _)| p)).flatten();
        self.other.jg10.poisons.get(&(actor, w)).copied().or(own)
    }

    /// The navmesh obstacles scripts added (AddNavmeshObstacle), for the engine.
    pub fn script_nav_obstacles(&self) -> &BTreeSet<FormId> {
        &self.other.jg10.nav_obstacles
    }

    /// PathToPoint and PathToRef requests since the last call, for the engine.
    pub fn take_path_requests(&mut self) -> Vec<PathRequest> {
        std::mem::take(&mut self.other.jg10.paths)
    }

    /// EjectCasing requests since the last call, for the engine.
    pub fn take_casing_requests(&mut self) -> Vec<CasingRequest> {
        std::mem::take(&mut self.other.jg10.casings)
    }

    /// Whether a shot's muzzle flash gives its light (DisableMuzzleFlashLights).
    pub fn muzzle_light_on(&self, shooter: FormId) -> bool {
        match self.other.jg10.muzzle_lights_off {
            1 => false,
            2 => shooter != FormId::PLAYER_REF,
            3 => shooter == FormId::PLAYER_REF,
            _ => true,
        }
    }

    /// The scope picture a script set (`Some("")`: cleared) and whether scripts hid it.
    pub fn script_scope(&self) -> (Option<&str>, Option<bool>) {
        (self.other.jg10.scope_model.as_deref(), self.other.jg10.scope_visible)
    }

    /// The highlighted actor and body part (HighlightBodyPartAlt), for VATS's drawing.
    pub fn body_part_highlight(&self) -> Option<(FormId, i64)> {
        self.other.jg10.highlight
    }

    /// RemapLand's swaps, for the terrain.
    pub fn land_remaps(&self) -> &BTreeMap<FormId, (FormId, i32, i32)> {
        &self.other.jg10.land_remaps
    }

    /// ShowBarberMenuEx's filter while its barber menu is open: whether a hair (`beard`
    /// false) or beard may be offered. 0 blacklists both, 1 blacklists beards and
    /// whitelists hairs, 2 the reverse, 3 whitelists both [documented].
    pub fn barber_allows(&self, part: FormId, beard: bool) -> bool {
        let Some((flag, list)) = self.other.jg10.barber_filter else { return true };
        let white = if beard { flag & 2 != 0 } else { flag & 1 != 0 };
        let listed = crate::functions::refs::list_members(self, list).contains(&part);
        listed == white
    }

    /// TriggerScreenSplatterEx's textures, for the engine's splats.
    pub fn splatter_textures(&self) -> Option<&SplatterTextures> {
        self.other.jg10.splatter_textures.as_ref()
    }

    /// UpdateCrosshairPrompt's and UpdateRepairMenu's call counts (the engine redraws on
    /// each new count).
    pub fn ui_refreshes(&self) -> (u64, u64) {
        (self.other.jg10.prompt_refreshes, self.other.jg10.repair_refreshes)
    }

    /// A FaceGen value list of an actor base: the player's from the character creator's
    /// look, else a script's change or the record.
    fn facegen_values(&self, base: FormId, kind: usize) -> Vec<f64> {
        let (sig, n) = FACEGEN[kind];
        if base == FormId::PLAYER_BASE {
            if let Some(l) = self.player.look.as_ref() {
                let v = [&l.fggs, &l.fgga, &l.fgts][kind];
                return v.iter().map(|&x| f64::from(x)).collect();
            }
        }
        let rec: Vec<f64> = self.data.record_field(self.unclone(base), sig, "").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
        (0..n.max(rec.len()))
            .map(|i| match self.field_overrides.get(&(base, format!("{sig}/{i}"))) {
                Some(XVal::Num(x)) => *x,
                _ => rec.get(i).copied().unwrap_or(0.0),
            })
            .collect()
    }
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
        Some(XVal::Num(x)) if x.is_finite() => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k)? {
        XVal::Str(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: Vec<f64>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), XVal::Num(v))).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn pos(g: &Game, r: FormId) -> Option<[f32; 3]> {
    if r == FormId::PLAYER_REF {
        return g.world.peek(r).map(|s| s.pos);
    }
    crate::awareness::position(g, r)
}

/// A form's record type (a clone's original's).
fn sig(g: &Game, f: FormId) -> Option<[u8; 4]> {
    g.data.sig(g.unclone(f))
}

fn is_actor(g: &Game, r: FormId) -> bool {
    r == FormId::PLAYER_REF || g.itr_is_actor(r)
}

/// The JohnnyGuitar batch-10 functions; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- navmeshes and paths ----
        // The calling reference joins or leaves the obstacle list [documented]; the
        // engine's navmesh cuts round it (spec note §3). Returns 1 both on a reference
        // [observed: jg10.returns] and without a calling reference [observed: Fallout:
        // New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24,
        // `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // `w4c_b3_10_jg10_safe_edges`: `obsNoRef=1/1`].
        "addnavmeshobstacle" | "removenavmeshobstacle" => {
            if let Some(r) = subject {
                if name == "addnavmeshobstacle" {
                    g.other.jg10.nav_obstacles.insert(r);
                } else {
                    g.other.jg10.nav_obstacles.remove(&r);
                }
            }
            XVal::Num(1.0)
        }
        // `x y z skipDisabled [zLimit]`: [x, y, z, distance] of the nearest triangle's
        // centre, or of the triangle under the point (GetPointInNavMesh) [documented];
        // an empty array when there is none [hypothesis] (`crate::nav_ask`).
        "getnearestnavmeshtriangle" | "getpointinnavmesh" => {
            let at = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            let z = Some(num(args, 4, 0.0) as f32).filter(|z| *z > 0.0);
            let under = name == "getpointinnavmesh";
            match crate::nav_ask::triangle(at, under, num(args, 3, 0.0) != 0.0, z) {
                Some(c) => {
                    let d = ((c[0] - at[0]).powi(2) + (c[1] - at[1]).powi(2) + (c[2] - at[2]).powi(2)).sqrt();
                    list(g, vec![f64::from(c[0]), f64::from(c[1]), f64::from(c[2]), f64::from(d)])
                }
                None => list(g, Vec::new()),
            }
        }
        // The actor walks to the point or reference, its package kept [documented]; 1
        // when the request is made, 0 for no actor or target [hypothesis: the page gives
        // a float and no meaning].
        "pathtopoint" | "pathtoref" => {
            let Some(a) = subject.filter(|&a| is_actor(g, a)) else { return Some(Ok(zero)) };
            let req = if name == "pathtopoint" {
                let p = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
                PathRequest { actor: a, to: PathTarget::Point(p), radius: num(args, 3, 0.0).max(0.0) as f32, angle: args.get(4).map(|_| num(args, 4, 0.0) as f32), face: false }
            } else {
                let Some(t) = form(args, 0).filter(|&t| pos(g, t).is_some()) else { return Some(Ok(zero)) };
                PathRequest { actor: a, to: PathTarget::Ref(t), radius: num(args, 1, 0.0).max(0.0) as f32, angle: None, face: num(args, 2, 0.0) != 0.0 }
            };
            g.other.jg10.paths.retain(|p| p.actor != a);
            g.other.jg10.paths.push(req);
            XVal::Num(1.0)
        }
        // ---- weapons ----
        // `[poison]` on an actor's equipped weapon or a weapon's inventory reference; none
        // or a null form takes it off [documented]. Requires an ALCH poison (`Game::is_poison`:
        // Hostile + Detrimental effects); applying a poison or clearing with omitted arg
        // returns 1, while non-poison ALCH (Stimpak) returns 0 and leaves the weapon
        // unpoisoned [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
        // JohnnyGuitarNVSE 528 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse`,
        // `w4h_09_apply_weapon_poison_stimpak` (`wave4h_reconcile.json`, #3803):
        // `PROBE|w4h_09_apply_weapon_poison_stimpak|eq=00004326|isPois(b=1,s=1,st=0)|init=00000000|bleak(1:001613D0)|clr1(1:00000000)|stim(0:00000000)|silver(1:00167475)|clr2(1:00000000)`],
        // and `w6t2p2_27_jg10_apply_poison_and_muzzle_hex`
        // (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
        // `unarm=0,ok=1,bad=0,kept=1,clr=1` confirming unarmed returns `0`, non-`ALCH`
        // (`Weap10mmPistol 0x434F`) returns `0` while keeping the already-applied poison,
        // and omitted argument clears returning `1`].
        "applyweaponpoison" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let key = match g.temp_item(r) {
                Some((c, item)) => (c, item),
                None if is_actor(g, r) => match crate::functions::items::equipped_weapon(g, r) {
                    Some(w) => (r, w),
                    None => return Some(Ok(zero)),
                },
                None => return Some(Ok(zero)),
            };
            if sig(g, key.1) != Some(*b"WEAP") {
                return Some(Ok(zero));
            }
            match form(args, 0) {
                Some(p) if g.is_poison(p) => {
                    g.other.jg10.poisons.insert(key, p);
                    XVal::Num(1.0)
                }
                Some(_) => zero,
                None => {
                    g.other.jg10.poisons.remove(&key);
                    XVal::Num(1.0)
                }
            }
        }
        // `[node] [model]`: a casing from the reference's weapon [documented].
        "ejectcasing" => {
            if let Some(a) = subject.filter(|&a| is_actor(g, a)) {
                g.other.jg10.casings.push(CasingRequest { actor: a, node: text(args, 0), model: text(args, 1) });
            }
            zero
        }
        // `[mode]`: 0 on again, 1 off for all, 2 the player's, 3 NPCs' [documented]; a
        // mode outside 0–3 leaves the setting unchanged and returns exact `u32::MAX`
        // (`4_294_967_295`, `0xFFFFFFFF`, printed as `FFFFFFFF` under `%x` and
        // `4.29497e+09` under `%g`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE
        // 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
        // 10 official base masters, `w4c_b3_10_jg10_safe_edges` (`dm(4.29497e+09,0)`),
        // `w4h_06_disable_muzzle_flash_hex`
        // (`tools/capture/nvse/catalogs/wave4h_reconcile.json`, #3803):
        // `PROBE|w4h_06_disable_muzzle_flash_hex|dm99(hex=FFFFFFFF,i=00000000,g=4.29497e+09)|dm1(1,1)|dm0(0,0)|dt(c1=1,ex1=1,c2=-1,ex2=1,ex3=0)|getOps(0,0,0)`,
        // and `w6t2p2_27_jg10_apply_poison_and_muzzle_hex`
        // (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
        // `muz=4294967295,neg=4.29497e+09`; zero DLL, PDB, or third-party source
        // consulted].
        "disablemuzzleflashlights" => {
            let m = num(args, 0, 0.0);
            if (0.0..=3.0).contains(&m) && m.fract() == 0.0 {
                g.other.jg10.muzzle_lights_off = m as u8;
                zero
            } else {
                XVal::Num(f64::from(u32::MAX))
            }
        }
        // `path [weapon]`: the scope's picture; a weapon gives its own (MOD3) and the path
        // is ignored [documented].
        "setweaponscopeuimodel" => {
            let model = match form(args, 1) {
                Some(w) => crate::nvse::record_value(g, w, "MOD3", "model").or_else(|| crate::nvse::record_value(g, w, "MOD3", "")).and_then(|v| match v {
                    XVal::Str(s) => Some(s),
                    _ => None,
                }),
                None => text(args, 0),
            };
            if let Some(m) = model {
                g.other.jg10.scope_model = Some(m);
            }
            zero
        }
        // `visible`: 0 hides the scope, 1 shows it [documented].
        "toggleweaponscopeuimodel" => {
            g.other.jg10.scope_visible = Some(num(args, 0, 0.0) != 0.0);
            zero
        }
        // Hides the scope and takes its picture away [documented].
        "clearweaponscopeuimodel" => {
            g.other.jg10.scope_model = Some(String::new());
            g.other.jg10.scope_visible = Some(false);
            zero
        }
        // `weapon`: its scope model path (MOD3) goes; 1 when it had one [documented:
        // "isSuccess"; hypothesis: a weapon without one fails].
        "removescopemodelpath" => {
            let Some(w) = form(args, 0).filter(|&w| sig(g, w) == Some(*b"WEAP")) else { return Some(Ok(zero)) };
            let had = ["", "model"].iter().any(|p| matches!(crate::nvse::record_value(g, w, "MOD3", p), Some(XVal::Str(s)) if !s.is_empty()));
            if had {
                for p in ["", "model"] {
                    g.field_overrides.insert((w, format!("MOD3/{p}")), XVal::Str(String::new()));
                }
            }
            yes(had)
        }
        // ---- FaceGen ----
        // `base type n`: the base's nth value of the list (FACEGEN) [documented: the
        // arguments; hypothesis: the types; observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, GSDocMitchellHouse
        // (0x00103DF9), 10 official base masters, w4d_31_jg_facegen_bounds
        // (tools/capture/nvse/catalogs/wave4d_extenders.json):
        // t0(0=-0.2281,49=0.0000)|t1(0=0.0000) on Doc Mitchell (0x00104C0C); no DLL or
        // source consulted]. Returning 0.0 for out-of-range indices (>= 50 / >= 30 /
        // < 0) or invalid types (>= 3) is [design: crash-safety guard avoiding
        // out-of-bounds reads, which crash/hang JohnnyGuitarNVSE 528].
        "facegengetnthproperty" => {
            let (Some(b), Ok(k), Ok(i)) = (form(args, 0), usize::try_from(num(args, 1, -1.0) as i64), usize::try_from(num(args, 2, -1.0) as i64)) else { return Some(Ok(zero)) };
            if k >= FACEGEN.len() {
                return Some(Ok(zero));
            }
            XVal::Num(g.facegen_values(b, k).get(i).copied().unwrap_or(0.0))
        }
        // `base type n value`: sets it, the player's in the creator's look (drawn again),
        // an NPC's as the base's record value (FaceGenRefreshAppearance draws it); invalid
        // type returns 0, while a valid type with an out-of-bounds index leaves data
        // unchanged and returns 1 [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
        // JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official
        // base masters, `w4c_b3_10_jg10_safe_edges`: `fg(badT=0,oob=1)`].
        "facegensetnthproperty" => {
            let (Some(b), Ok(k), Ok(i)) = (form(args, 0), usize::try_from(num(args, 1, -1.0) as i64), usize::try_from(num(args, 2, -1.0) as i64)) else { return Some(Ok(zero)) };
            let Some(v) = crate::nvse::field_number(num(args, 3, f64::NAN), crate::nvse::F32.0, crate::nvse::F32.1) else { return Some(Ok(zero)) };
            let Some(&(sub, n)) = FACEGEN.get(k) else { return Some(Ok(zero)) };
            if sig(g, b) != Some(*b"NPC_") {
                return Some(Ok(zero));
            }
            if i >= n {
                return Some(Ok(XVal::Num(1.0)));
            }
            if b == FormId::PLAYER_BASE {
                if let Some(l) = g.player.look.as_mut() {
                    let list = match k { 0 => &mut l.fggs, 1 => &mut l.fgga, _ => &mut l.fgts };
                    if list.len() < n {
                        list.resize(n, 0.0);
                    }
                    list[i] = v as f32;
                    g.appearance_changes.push(crate::appearance::AppearanceChange { actor: FormId::PLAYER_REF, reset: false });
                    return Some(Ok(XVal::Num(1.0)));
                }
            }
            g.field_overrides.insert((b, format!("{sub}/{i}")), XVal::Num(v));
            XVal::Num(1.0)
        }
        // The reference's face drawn again [documented].
        "facegenrefreshappearance" => {
            if let Some(a) = subject.filter(|&a| is_actor(g, a)) {
                g.appearance_changes.push(crate::appearance::AppearanceChange { actor: a, reset: false });
            }
            zero
        }
        // ---- VATS highlights ----
        // `[part]` 0–15 on the calling actor, one actor at a time; none (or 15) is
        // everything but the weapon [documented]; a part outside 0–15 is not taken
        // [hypothesis].
        "highlightbodypartalt" => {
            let Some(a) = subject.filter(|&a| is_actor(g, a)) else { return Some(Ok(zero)) };
            let p = args.first().map_or(15.0, |_| num(args, 0, -1.0));
            if (0.0..=15.0).contains(&p) && p.fract() == 0.0 {
                g.other.jg10.highlight = Some((a, if p == 15.0 { -1 } else { p as i64 }));
            }
            zero
        }
        // Every highlight goes, whoever calls it [documented].
        "deactivateallhighlightsalt" => {
            g.other.jg10.highlight = None;
            zero
        }
        // The calling reference's highlight goes [hypothesis: the page is empty; read from
        // its name].
        "removehighlightedref" => {
            if g.other.jg10.highlight.is_some_and(|(a, _)| Some(a) == subject) {
                g.other.jg10.highlight = None;
            }
            zero
        }
        // ---- land ----
        // The land texture (LTEX) under the reference; nothing indoors [documented], but
        // the original gave LRockCanyonRubbleDirt01 inside Doc Mitchell's house [observed:
        // jg10.underfeet, 2026-11-09], taken as the last texture found under the reference
        // outside [hypothesis]. 0 where the default texture shows, or with none found yet
        // [hypothesis].
        "getlandtextureunderfeet" => {
            let Some(r) = subject else { return Some(Ok(XVal::Ref(0))) };
            let Some(p) = pos(g, r) else { return Some(Ok(XVal::Ref(0))) };
            let from = [p[0], p[1], p[2] + UNDERFOOT_UP];
            let to = [p[0], p[1], p[2] - UNDERFOOT_DOWN];
            // No terrain under it (indoors): the last texture found outside; terrain
            // showing the default texture: 0.
            match crate::ray::cast_hit(from, to, crate::ray::CastKind::LandTexture) {
                Some(h) => {
                    let f = h.form.unwrap_or(0);
                    g.other.jg10.last_land_texture.insert(r, f);
                    XVal::Ref(f)
                }
                None => XVal::Ref(g.other.jg10.last_land_texture.get(&r).copied().unwrap_or(0)),
            }
        }
        // `land world x y`: the LAND takes the cell's landscape; `land` alone puts it back;
        // 1 done, 0 for a target that is no LAND or a source cell that does not exist
        // [documented: the return 0/1 and the reset; hypothesis: when it fails].
        "remapland" => {
            // The game's data does not keep LAND records' types (`GameData::sig`), so any form
            // of no other known type is taken [hypothesis].
            let Some(land) = form(args, 0).filter(|&l| sig(g, l).is_none_or(|s| &s == b"LAND")) else { return Some(Ok(zero)) };
            if args.len() < 4 {
                return Some(Ok(yes(g.other.jg10.land_remaps.remove(&land).is_some())));
            }
            let Some(world) = form(args, 1) else { return Some(Ok(zero)) };
            let (x, y) = (num(args, 2, 0.0) as i32, num(args, 3, 0.0) as i32);
            if !g.world.placed.world_grid.get(&world).is_some_and(|grid| grid.contains_key(&(x, y))) {
                return Some(Ok(zero));
            }
            g.other.jg10.land_remaps.insert(land, (world, x, y));
            XVal::Num(1.0)
        }
        // ---- idles ----
        // `[stop]`: the actor picks its own idles again after PlayIdle; 1 stops the one
        // playing at once [documented]. NVForever's PlayIdle holds no override past its
        // idle, so the stop is what is left to do [hypothesis, spec note §1].
        "refreshidle" => {
            if let Some(a) = subject.filter(|&a| is_actor(g, a)) {
                if num(args, 0, 0.0) != 0.0 {
                    g.gaps.idle_stops.push(a);
                }
            }
            zero
        }
        // ---- menus and the screen ----
        // `flag list`: the barber menu, its hairs and beards black- or whitelisted by the
        // list [documented]; a flag outside 0–3 or no list opens it unfiltered
        // [hypothesis].
        "showbarbermenuex" => {
            let flag = num(args, 0, -1.0);
            g.other.jg10.barber_filter = form(args, 1).filter(|_| (0.0..=3.0).contains(&flag) && flag.fract() == 0.0).map(|l| (flag as u8, l));
            g.race_menu = crate::game::RaceMenu::Barber;
            g.race_menu_studio = false;
            g.open_menu(crate::game::menus::RACE);
            zero
        }
        // `count duration size opacity alpha colour flare fade`: TriggerScreenSplatter's
        // splats with the given textures [documented]; returns 1 even when `count == 0`
        // (without queuing a splatter trigger) [observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
        // 10 official base masters, `w4c_b3_10_jg10_safe_edges`: `spl0=1`].
        "triggerscreensplatterex" => {
            let count = num(args, 0, 0.0).clamp(0.0, f64::from(u32::MAX)) as u32;
            if count == 0 {
                return Some(Ok(XVal::Num(1.0)));
            }
            let duration = Some(num(args, 1, 0.0).min(3600.0) as f32).filter(|d| *d > 0.0);
            let factor = |i: usize| num(args, i, 1.0).clamp(0.0, crate::functions::misc::SPLATTER_MAX_FACTOR) as f32;
            g.ui.splatter.triggers.push(crate::functions::ui::SplatterTrigger { count, duration, size: factor(2), opacity: factor(3) });
            g.other.jg10.splatter_textures = match (text(args, 4), text(args, 5), text(args, 6)) {
                (Some(alpha), Some(color), Some(flare)) => Some(SplatterTextures { alpha, color, flare, fade: num(args, 7, 0.0) != 0.0 }),
                _ => None,
            };
            XVal::Num(1.0)
        }
        // The crosshair prompt is worked out again [documented].
        "updatecrosshairprompt" => {
            g.other.jg10.prompt_refreshes += 1;
            zero
        }
        // The Pip-Boy repair menu is worked out again [documented].
        "updaterepairmenu" => {
            g.other.jg10.repair_refreshes += 1;
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_facegen_lists_are_the_records_lengths() {
        assert_eq!(FACEGEN.map(|(_, n)| n), [50, 30, 50]);
    }
}
