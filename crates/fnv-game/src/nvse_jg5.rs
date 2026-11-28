//! JohnnyGuitar NVSE, batch 5 (#1007): alternate textures, armour FaceGen model flags and
//! icons, worldspace map textures and encounter zones, region map names and the location
//! name, camera shots, body part strings, explosion and projectile sounds, equip types,
//! recipe and weapon mod lookups, actor effect types and the time played [documented:
//! each page in the wiki export; docs/spec-notes/ext-jg5.md]. Setters change the record
//! for the session, read by the getters (and, for camera shots, by the kill camera).

use crate::camera_paths::{CameraShotDef, ShotAction, ShotActor};
use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

/// One alternate texture of a model (MODS, MO2S, MO3S, MO4S) [documented: schema
/// common.json AlternateTexture; docs/spec-notes/texture-swaps.md]: the shape's name, the
/// texture set put on it and the shape's 3D index.
#[derive(Debug, Clone, PartialEq)]
pub struct AltTexture {
    pub shape: String,
    pub texture_set: FormId,
    pub index: i32,
}

/// The session's changes of this batch.
#[derive(Debug, Default)]
pub struct Jg5State {
    /// A model's alternate textures a script changed, a whole copy of the record's, by
    /// form and subrecord (read through [`alt_textures`]).
    pub alt_textures: HashMap<(FormId, [u8; 4]), Vec<AltTexture>>,
    /// SetRegionMapName's map blocks: the name and the header's priority (read through
    /// [`region_map`]).
    pub region_maps: HashMap<FormId, (String, u8)>,
    /// The camera shots a script changed, whole copies of the record's (read through
    /// [`camera_shot`]).
    pub camera_shots: HashMap<FormId, CameraShotDef>,
    /// SetCameraShotTraitNumeric's codes for traits 0-2 as given, kept even when outside
    /// the field's table [observed: jg5, trait 1 set to 7 reads 7].
    pub shot_codes: HashMap<(FormId, i64), i64>,
}

/// When the process made its first game: GetTimePlayed counts from it.
static LAUNCH: OnceLock<Instant> = OnceLock::new();

/// Start GetTimePlayed's clock (`Game::new`); later calls keep the first time.
pub fn mark_launch() {
    LAUNCH.get_or_init(Instant::now);
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

/// A whole number at `k`, else none.
fn index(args: &[XVal], k: usize) -> Option<i64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.fract() == 0.0 => Some(*x as i64),
        _ => None,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k)? {
        XVal::Str(s) => Some(s.clone()),
        _ => None,
    }
}

/// The game's "script call failed": worded as the VM's NoReference, which stops the
/// script (as `nvse_jippp`'s ClampToGround does).
fn failed(name: &str) -> Option<Result<XVal, String>> {
    Some(Err(fnv_script::vm::VmError::NoReference(name.to_string()).to_string()))
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn string(v: Option<XVal>) -> XVal {
    match v {
        Some(XVal::Str(s)) => XVal::Str(s),
        _ => XVal::Str(String::new()),
    }
}

fn list(g: &mut Game, items: impl Iterator<Item = XVal>) -> XVal {
    let entries = items.enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// A reference's base form, a base form itself.
fn base(g: &Game, f: FormId) -> FormId {
    crate::functions::base_of(g, f).unwrap_or(f)
}

fn hex_form(v: Option<&Json>) -> Option<FormId> {
    v.and_then(Json::as_str).and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).filter(|&x| x != 0).map(FormId)
}

/// The alternate textures of `form`'s model `sub` as they stand now: a script's changes
/// for the session, else the record's, in record order.
pub fn alt_textures(g: &Game, form: FormId, sub: [u8; 4]) -> Vec<AltTexture> {
    if let Some(v) = g.other.jg5.alt_textures.get(&(form, sub)) {
        return v.clone();
    }
    let Some(Json::Array(rows)) = g.data.record_fields.get(&form).and_then(|f| f.iter().find(|(s, _)| *s == sub)).map(|(_, v)| v) else { return Vec::new() };
    rows.iter()
        .filter_map(|r| {
            let shape = r.get("shape_name").and_then(Json::as_str).unwrap_or_default().to_string();
            let index = r.get("shape_index").and_then(Json::as_i64)? as i32;
            Some(AltTexture { shape, texture_set: hex_form(r.get("texture_set"))?, index })
        })
        .collect()
}

/// The alternate-texture subrecord of a weapon's world model (MODS), or of an armour's
/// model 1 male biped (MODS), 2 female biped (MO3S), 3 male world (MO2S), 4 female world
/// (MO4S) [documented: GetArmorAltTextures's model IDs; the subrecords: the models'
/// order in ARMO, as `setbipedmodelpath` reads them (MODL, MOD3, MOD2, MOD4)].
fn alt_sub(g: &Game, f: FormId, model: Option<i64>) -> Option<[u8; 4]> {
    match (g.data.sig(f)?, model) {
        (s, _) if &s == b"WEAP" => Some(*b"MODS"),
        (s, Some(1)) if &s == b"ARMO" => Some(*b"MODS"),
        (s, Some(2)) if &s == b"ARMO" => Some(*b"MO3S"),
        (s, Some(3)) if &s == b"ARMO" => Some(*b"MO2S"),
        (s, Some(4)) if &s == b"ARMO" => Some(*b"MO4S"),
        _ => None,
    }
}

/// The model's alternate textures to change, copied from the record on the first change.
fn alt_mut<'a>(g: &'a mut Game, f: FormId, sub: [u8; 4]) -> &'a mut Vec<AltTexture> {
    if !g.other.jg5.alt_textures.contains_key(&(f, sub)) {
        let v = alt_textures(g, f, sub);
        g.other.jg5.alt_textures.insert((f, sub), v);
    }
    g.other.jg5.alt_textures.entry((f, sub)).or_default()
}

/// The encounter zone of a cell as it stands now: SetCellEncounterZone's (`nvse_jg2`),
/// else the cell's own (CELL XEZN), else SetWorldspaceEncounterZone's for its
/// worldspace, else the worldspace's (WRLD XEZN) (`fnv_sim::World::cell_zone_of`).
pub fn cell_zone(g: &Game, cell: FormId) -> Option<FormId> {
    g.world.cell_zone_of(cell)
}

/// A worldspace's encounter zone: SetWorldspaceEncounterZone's, else WRLD XEZN.
fn world_zone(g: &Game, w: FormId) -> Option<FormId> {
    g.world.world_zone_of(w)
}

/// A region's map block (its name and priority): SetRegionMapName's, else the record's
/// (RDAT type 4 + RDMP).
pub fn region_map(g: &Game, region: FormId) -> Option<(String, u8)> {
    g.other.jg5.region_maps.get(&region).cloned().or_else(|| g.data.regions.get(&region)?.map.clone())
}

/// A camera shot as it stands now: a script's changes for the session, else the
/// record's (CAMS). The kill camera reads this.
pub fn camera_shot<'a>(g: &'a Game, shot: FormId) -> Option<&'a CameraShotDef> {
    g.other.jg5.camera_shots.get(&shot).or_else(|| g.data.camera_shots.get(&shot))
}

fn shot_mut<'a>(g: &'a mut Game, shot: FormId) -> Option<&'a mut CameraShotDef> {
    if !g.other.jg5.camera_shots.contains_key(&shot) {
        let c = g.data.camera_shots.get(&shot)?.clone();
        g.other.jg5.camera_shots.insert(shot, c);
    }
    g.other.jg5.camera_shots.get_mut(&shot)
}

/// CAMS DATA's codes [documented: schema effects.json CameraShotData; GECK wiki
/// CameraShot].
fn action_code(a: ShotAction) -> u8 {
    match a {
        ShotAction::Shoot => 0,
        ShotAction::Fly => 1,
        ShotAction::Hit => 2,
        ShotAction::Zoom => 3,
    }
}

fn action_of(code: i64) -> Option<ShotAction> {
    Some(match code {
        0 => ShotAction::Shoot,
        1 => ShotAction::Fly,
        2 => ShotAction::Hit,
        3 => ShotAction::Zoom,
        _ => return None,
    })
}

fn actor_code(a: ShotActor) -> u8 {
    match a {
        ShotActor::Attacker => 0,
        ShotActor::Projectile => 1,
        ShotActor::Target => 2,
    }
}

fn actor_of(code: i64) -> Option<ShotActor> {
    Some(match code {
        0 => ShotActor::Attacker,
        1 => ShotActor::Projectile,
        2 => ShotActor::Target,
        _ => return None,
    })
}

/// The CELL DATA interior flag (bit 0).
pub(crate) fn interior(g: &Game, cell: FormId) -> bool {
    matches!(crate::nvse::record_value(g, cell, "DATA", ""), Some(XVal::Num(x)) if (x as u64) & 1 != 0)
}

/// The location name of a reference [documented: GetLocationName: "interior cell name,
/// worldspace name or a Region Map Data name"; observed: jg5's travel job, Doc Mitchell's
/// House indoors, Goodsprings outdoors, "Mojave Wasteland" in a cell whose regions have no
/// map data]: indoors the cell's name; outdoors the first map name among the regions the
/// cell is in, in CELL XCLR order [observed: a name SetRegionMapName gave Goodsprings'
/// first region won over VMapGoodspringsRegion's at priority 50; hypothesis: the list's
/// order decides, not the priority], else the worldspace's name.
/// The map name of the region the player stands in, outdoors only (the regions of the
/// player's cell, the first with a map name): the player's HUD Region Names tweak
/// [design: docs/design/tweaks.md].
pub fn player_region_name(g: &Game) -> Option<String> {
    let cell = g.cell_of(FormId::PLAYER_REF)?;
    if interior(g, cell) {
        return None;
    }
    g.cell_regions(cell).into_iter().find_map(|reg| region_map(g, reg).map(|m| m.0).filter(|n| !n.trim().is_empty()))
}

pub(crate) fn location_name(g: &Game, r: FormId) -> String {
    let Some(cell) = g.cell_of(r) else { return String::new() };
    let name = |f: FormId| match crate::nvse::record_value(g, f, "FULL", "") {
        Some(XVal::Str(s)) => s,
        _ => String::new(),
    };
    if interior(g, cell) {
        return name(cell);
    }
    match region_map_name(g, cell) {
        Some(n) => n,
        None => g.world.placed.world_of(cell).map(name).unwrap_or_default(),
    }
}

/// The first map name among the regions an exterior cell is in, in CELL XCLR order
/// ([`location_name`]).
pub(crate) fn region_map_name(g: &Game, cell: FormId) -> Option<String> {
    g.cell_regions(cell).into_iter().find_map(|reg| region_map(g, reg).map(|m| m.0).filter(|n| !n.is_empty()))
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let of_sig = |g: &Game, f: Option<FormId>, sig: &[u8; 4]| f.filter(|&f| g.data.sig(f) == Some(*sig));
    let field = |g: &Game, f: FormId, sub: &str, path: &str| crate::nvse::record_value(g, f, sub, path);
    let v = match name {
        // ---- alternate textures ----
        // `weapon`: the texture sets of the world model's alternate textures (MODS), in
        // record order [documented: GetWeaponAltTextures]; `armor model`, models 1-4
        // [documented: GetArmorAltTextures].
        "getweaponalttextures" | "getarmoralttextures" => {
            // Sorted by 3D index [observed: jg5, dynamite's record has 2, 0, 1 and the game
            // lists 0, 1, 2]; an armour model outside 1-4 gives no array [observed: jg5,
            // models 0 and 5 read ar_Size -1].
            let f = form(args, 0);
            if name == "getarmoralttextures" && of_sig(g, f, b"ARMO").is_some() && !(1..=4).contains(&index(args, 1).unwrap_or(0)) {
                return Some(Ok(XVal::Array(0)));
            }
            let sub = f.and_then(|f| alt_sub(g, f, index(args, 1)).filter(|_| (name == "getweaponalttextures") == (g.data.sig(f) == Some(*b"WEAP"))));
            let mut rows = match (f, sub) {
                (Some(f), Some(sub)) => alt_textures(g, f, sub),
                _ => Vec::new(),
            };
            rows.sort_by_key(|a| a.index);
            list(g, rows.into_iter().map(|a| XVal::Ref(a.texture_set.0)))
        }
        // `form [model]`: a map from each 3D index to its texture set [documented:
        // GetAltTexturesEx]; an armour without a model ID reads model 3, the male world
        // model [observed: jg5, MS18RangerBattlearmor and the radiation suit], and a model
        // outside 1-4 gives no array [observed: jg5, models 0 and 5].
        "getalttexturesex" => {
            let f = form(args, 0);
            let model = index(args, 1).unwrap_or(3);
            if of_sig(g, f, b"ARMO").is_some() && !(1..=4).contains(&model) {
                return Some(Ok(XVal::Array(0)));
            }
            let rows: Vec<AltTexture> = f.and_then(|f| Some((f, alt_sub(g, f, Some(model))?))).map(|(f, s)| alt_textures(g, f, s)).unwrap_or_default();
            let entries = rows.iter().map(|a| (XKey::Num(f64::from(a.index)), XVal::Ref(a.texture_set.0))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::Map, entries))
        }
        // `weapon index set` / `armor model index set`: the alternate texture of that 3D
        // index takes the texture set, or a new one is added for it [documented:
        // SetWeaponAltTexture, SetArmorAltTexture and their notes]; 1 when done. A new
        // entry has no shape name, and a form that is not a texture set changes nothing
        // (see docs/spec-notes/ext-jg5.md).
        "setweaponalttexture" | "setarmoralttexture" => {
            let armor = name == "setarmoralttexture";
            let f = of_sig(g, form(args, 0), if armor { b"ARMO" } else { b"WEAP" });
            let k = usize::from(armor);
            let sub = f.and_then(|f| alt_sub(g, f, if armor { index(args, 1) } else { None }));
            let (Some(f), Some(sub), Some(at), Some(set)) = (f, sub, index(args, k + 1), of_sig(g, form(args, k + 2), b"TXST")) else { return Some(Ok(zero)) };
            let Ok(at) = i32::try_from(at) else { return Some(Ok(zero)) };
            let rows = alt_mut(g, f, sub);
            match rows.iter_mut().find(|a| a.index == at) {
                Some(a) => a.texture_set = set,
                None => rows.push(AltTexture { shape: String::new(), texture_set: set, index: at }),
            }
            XVal::Num(1.0)
        }
        // `weapon index`: that 3D index's alternate texture removed, every one for -1
        // [documented: ClearWeaponAltTexture]; -1 returns 1 even when no overrides exist,
        // while a specific empty index returns 0 [observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
        // 10 official base masters, `w4c_b6_06_clear_weap_alt_tex`: `emp5=0,all=1`].
        "clearweaponalttexture" => {
            let (Some(f), Some(at)) = (of_sig(g, form(args, 0), b"WEAP"), index(args, 1)) else { return Some(Ok(zero)) };
            if at == -1 {
                alt_mut(g, f, *b"MODS").clear();
                return Some(Ok(XVal::Num(1.0)));
            }
            if alt_textures(g, f, *b"MODS").iter().all(|a| i64::from(a.index) != at) {
                return Some(Ok(zero));
            }
            alt_mut(g, f, *b"MODS").retain(|a| i64::from(a.index) != at);
            XVal::Num(1.0)
        }
        // ---- armour ----
        // `armor flag female [on]`: the FaceGen model flags, 0 head, 1 torso, 2 right
        // hand, 3 left hand, of the male (MODD) or female (MOSD) biped model [documented:
        // Get/SetFacegenModelFlag; the subrecords: schema common.json].
        "getfacegenmodelflag" | "setfacegenmodelflag" => {
            let f = form(args, 0).or(subject).map(|f| base(g, f));
            let (Some(f), Some(bit @ 0..=3)) = (of_sig(g, f, b"ARMO"), index(args, 1)) else { return Some(Ok(zero)) };
            let sub = if num(args, 2, 0.0) != 0.0 { "MOSD" } else { "MODD" };
            let flags = match field(g, f, sub, "") {
                Some(XVal::Num(x)) => x as u64,
                _ => 0,
            };
            if name == "getfacegenmodelflag" {
                yes(flags & (1 << bit) != 0)
            } else {
                let new = if num(args, 3, 0.0) != 0.0 { flags | 1 << bit } else { flags & !(1 << bit) };
                g.field_overrides.insert((f, format!("{sub}/")), XVal::Num(new as f64));
                zero
            }
        }
        // `path female form`: the male (ICON) or female (ICO2) icon, read by
        // GetBipedIconPath [documented: SetBipedIconPathAlt].
        "setbipediconpathalt" => {
            let f = form(args, 2).or(subject).map(|f| base(g, f));
            if let (Some(path), Some(f)) = (text(args, 0), f) {
                let sub = if num(args, 1, 0.0) != 0.0 { "ICO2" } else { "ICON" };
                g.field_overrides.insert((f, format!("{sub}/")), XVal::Str(path));
            }
            zero
        }
        // ---- worldspaces ----
        // `world [path]`: the map texture (WRLD ICON), under Data\Textures [documented:
        // Get/SetWorldSpaceMapTexture; observed: WastelandNV's
        // interface\worldmap\wasteland_nv_1024_no_map.dds].
        "getworldspacemaptexture" => string(of_sig(g, form(args, 0), b"WRLD").and_then(|w| field(g, w, "ICON", ""))),
        "setworldspacemaptexture" => {
            if let (Some(w), Some(path)) = (of_sig(g, form(args, 0), b"WRLD"), text(args, 1)) {
                g.field_overrides.insert((w, "ICON/".into()), XVal::Str(path));
            }
            zero
        }
        // `world`: its encounter zone [documented: GetWorldspaceEncounterZone, alias
        // GetWorldZone]; `world [zone]`, none removing it, 1 when done [documented:
        // SetWorldspaceEncounterZone, alias SetWorldZone]. The cells that name no zone of
        // their own take it (`cell_zone`).
        "getworldspaceencounterzone" | "getworldzone" => XVal::Ref(of_sig(g, form(args, 0), b"WRLD").and_then(|w| world_zone(g, w)).map_or(0, |z| z.0)),
        "setworldspaceencounterzone" | "setworldzone" => {
            let Some(w) = of_sig(g, form(args, 0), b"WRLD") else { return Some(Ok(zero)) };
            let z = form(args, 1).filter(|z| g.world.forms.zones.contains_key(z));
            if form(args, 1).is_some() && z.is_none() {
                return Some(Ok(zero));
            }
            g.world.set_world_zone(w, z);
            XVal::Num(1.0)
        }
        // ---- regions ----
        // `region`: the map name (RDMP) [documented: GetRegionMapName]; a region without
        // map data fails the call [observed: jg5, 0x123CE3, "script call failed"] (a non-region
        // also fails; see docs/spec-notes/ext-jg5.md). `region name`: a region without map
        // data gains it [documented: SetRegionMapName; observed: jg5], at priority 0
        // (see docs/spec-notes/ext-jg5.md); 1 when done [observed: jg5].
        "getregionmapname" => match of_sig(g, form(args, 0), b"REGN").and_then(|r| region_map(g, r)) {
            Some(m) => XVal::Str(m.0),
            None => return failed(name),
        },
        "setregionmapname" => {
            let (Some(r), Some(n)) = (of_sig(g, form(args, 0), b"REGN"), text(args, 1)) else { return Some(Ok(zero)) };
            let priority = region_map(g, r).map_or(0, |m| m.1);
            g.other.jg5.region_maps.insert(r, (n, priority));
            XVal::Num(1.0)
        }
        // `ref.GetLocationName`; without a reference the call fails [observed: jg5,
        // "script call failed"]; "" for no name [documented: the page's note on 5.18].
        "getlocationname" => match subject {
            Some(r) => XVal::Str(location_name(g, r)),
            None => return failed(name),
        },
        // ---- sounds ----
        // `explosion which sound`: 1 sound 1, 2 sound 2 (EXPL DATA) [documented:
        // SetExplosionSound], read by GetExplosionTraitForm; `projectile which sound`:
        // 1 the projectile's sound, 2 countdown, 3 disable (PROJ DATA) [documented:
        // SetProjectileSound]. A form that is not a sound changes nothing [hypothesis].
        "setexplosionsound" => {
            let f = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"EXPL"));
            let key = match index(args, 1) {
                Some(1) => "DATA/sound_1",
                Some(2) => "DATA/sound_2",
                _ => "",
            };
            let s = form(args, 2).filter(|&s| g.data.sig(g.unclone(s)) == Some(*b"SOUN"));
            if let (Some(f), Some(s), false) = (f, s, key.is_empty()) {
                g.field_overrides.insert((f, key.into()), XVal::Ref(s.0));
            }
            zero
        }
        "setprojectilesound" => {
            let p = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"PROJ"));
            let s = form(args, 2).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"SOUN"));
            let key = match index(args, 1) {
                Some(1) => "DATA/sound",
                Some(2) => "DATA/sound_countdown",
                Some(3) => "DATA/sound_disable",
                _ => "",
            };
            if let (Some(p), Some(s), false) = (p, s, key.is_empty()) {
                g.field_overrides.insert((p, key.into()), XVal::Ref(s.0));
            }
            zero
        }
        // ---- items ----
        // `item type`: the equip type (ETYP), a reference's base's, read by GetEquipType
        // [documented: SetEquipType and the Equip Type table]; 0-13 are taken and give 1,
        // -1 (none) and anything past 13 change nothing and give 0 [observed: jg5, the
        // Stimpak set to 5, 40 and -1].
        "setequiptype" => {
            let f = form(args, 0).or(subject).map(|f| base(g, f));
            let (Some(f), Some(t @ 0..=13)) = (f, index(args, 1)) else { return Some(Ok(zero)) };
            g.field_overrides.insert((f, "ETYP/".into()), XVal::Num(t as f64));
            XVal::Num(1.0)
        }
        // `category`: its flags, 1 a subcategory (RCCT DATA) [documented:
        // GetRecipeCategoryFlags].
        "getrecipecategoryflags" => XVal::Num(form(args, 0).and_then(|c| g.data.recipe_categories.get(&c)).map_or(0.0, |c| f64::from(u8::from(c.subcategory)))),
        // `item`: the recipes whose outputs hold the item, in FormID order [documented:
        // GetFormRecipesAlt; hypothesis: the order, as GetFormRecipes].
        "getformrecipesalt" => {
            let item = form(args, 0);
            let mut ids: Vec<FormId> = g.data.recipes.keys().copied().collect();
            ids.sort();
            let out: Vec<FormId> = match item {
                Some(item) => ids.into_iter().filter(|&id| crate::nvse_lutana::recipe(g, id).is_some_and(|r| r.outputs.iter().any(|e| e.0 == item))).collect(),
                None => Vec::new(),
            };
            list(g, out.into_iter().map(|r| XVal::Ref(r.0)))
        }
        // `mod`: the weapons whose mod slots (WMI1-3) hold it, in FormID order
        // [documented: GetWeaponsForMod; hypothesis: the order (native `w0=00176E5B` is not
        // ascending FormID)]; non-IMOD returns ar_Null (`ar_Size == -1`) and valid IMOD
        // `0x129878` returns 3 weapons [observed: Fallout: New Vegas 1.4.0.525 + xNVSE
        // 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
        // 10 official base masters, `w4c_b3_02_region_loc_weapmod` (`badModN=-1`) and
        // `w6t2_30_jg5_get_weapons_for_mod_and_armor_alt_tex`
        // (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
        // `wMod(n=3,w0=00176E5B,bad=-1)`].
        "getweaponsformod" => {
            let Some(m) = of_sig(g, form(args, 0), b"IMOD") else { return Some(Ok(XVal::Array(0))) };
            let mut ws: Vec<FormId> = g
                .data
                .sigs
                .iter()
                .filter(|(_, s)| *s == b"WEAP")
                .map(|(&w, _)| w)
                .filter(|&w| ["WMI1", "WMI2", "WMI3"].iter().any(|s| matches!(field(g, w, s, ""), Some(XVal::Ref(r)) if r == m.0)))
                .collect();
            ws.sort();
            list(g, ws.into_iter().map(|w| XVal::Ref(w.0)))
        }
        // `effect`: the actor effect's type (SPEL SPIT), 0 actor effect, 1 disease,
        // 2 power, 3 lesser power, 4 ability, 5 poison, 10 addiction; -1 for anything
        // else [documented: GetActorEffectType].
        "getactoreffecttype" => XVal::Num(form(args, 0).and_then(|s| g.spell_type(s)).map_or(-1.0, |t| t as f64)),
        // ---- camera shots ----
        // `shot [flags]`: CAMS DATA flags, bits 0-5 [documented: Get/SetCameraShotFlags].
        "getcamerashotflags" => XVal::Num(form(args, 0).and_then(|s| camera_shot(g, s)).map_or(0.0, |c| f64::from(c.flags))),
        "setcamerashotflags" => {
            let v = num(args, 1, f64::NAN);
            if let (Some(c), Some(v)) = (form(args, 0).and_then(|s| shot_mut(g, s)), crate::nvse::field_number(v.trunc(), crate::nvse::U32.0, crate::nvse::U32.1)) {
                c.flags = v as u32;
            }
            zero
        }
        // `shot [modifier]`: the image space modifier (MNAM), none clearing it
        // [documented: Get/SetCameraShotImageSpaceModifier]; a form that is not one
        // changes nothing [hypothesis].
        "getcamerashotimagespacemodifier" => XVal::Ref(form(args, 0).and_then(|s| camera_shot(g, s)).and_then(|c| c.imod).map_or(0, |m| m.0)),
        "setcamerashotimagespacemodifier" => {
            let m = form(args, 1);
            let ok = m.is_none() || of_sig(g, m, b"IMAD").is_some();
            if let (true, Some(c)) = (ok, form(args, 0).and_then(|s| shot_mut(g, s))) {
                c.imod = m;
            }
            zero
        }
        // `shot trait [value]`: 0 action, 1 location, 2 target (their codes), 3-5 the
        // player's, target's and global time multipliers, 6 max time, 7 min time, 8 target
        // % between actors [documented: Get/SetCameraShotTraitNumeric's notes; observed:
        // jg5, CamDefaultShoot]. The setter gives 1 when it set the trait [observed: jg5,
        // traits 1 and 6]; a code is kept as given even outside the field's table
        // [observed: jg5, location 7 reads 7], the kill camera keeping the shot's last
        // valid one; a number that is not finite changes nothing (see docs/spec-notes/ext-jg5.md).
        "getcamerashottraitnumeric" => {
            let (shot, t) = (form(args, 0), index(args, 1));
            if let Some(&code) = shot.zip(t).and_then(|k| g.other.jg5.shot_codes.get(&k)) {
                return Some(Ok(XVal::Num(code as f64)));
            }
            let c = shot.and_then(|s| camera_shot(g, s));
            XVal::Num(match (c, t) {
                (Some(c), Some(0)) => f64::from(action_code(c.action)),
                (Some(c), Some(1)) => f64::from(actor_code(c.location)),
                (Some(c), Some(2)) => f64::from(actor_code(c.target)),
                (Some(c), Some(3)) => crate::nvse::record_float(f64::from(c.time_player)),
                (Some(c), Some(4)) => crate::nvse::record_float(f64::from(c.time_target)),
                (Some(c), Some(5)) => crate::nvse::record_float(f64::from(c.time_global)),
                (Some(c), Some(6)) => crate::nvse::record_float(f64::from(c.max_time)),
                (Some(c), Some(7)) => crate::nvse::record_float(f64::from(c.min_time)),
                (Some(c), Some(8)) => crate::nvse::record_float(f64::from(c.target_pct)),
                _ => 0.0,
            })
        }
        "setcamerashottraitnumeric" => {
            let (t, v) = (index(args, 1), num(args, 2, f64::NAN));
            let float = crate::nvse::field_number(v, crate::nvse::F32.0, crate::nvse::F32.1).map(|x| x as f32);
            let code = (v.is_finite() && v.fract() == 0.0).then_some(v as i64);
            let shot = form(args, 0);
            let done = match t {
                Some(0..=2) => code.is_some(),
                Some(3..=8) => float.is_some(),
                _ => false,
            };
            if let Some(c) = shot.and_then(|s| shot_mut(g, s)) {
                match t {
                    Some(0) => c.action = code.and_then(action_of).unwrap_or(c.action),
                    Some(1) => c.location = code.and_then(actor_of).unwrap_or(c.location),
                    Some(2) => c.target = code.and_then(actor_of).unwrap_or(c.target),
                    Some(3) => c.time_player = float.unwrap_or(c.time_player),
                    Some(4) => c.time_target = float.unwrap_or(c.time_target),
                    Some(5) => c.time_global = float.unwrap_or(c.time_global),
                    Some(6) => c.max_time = float.unwrap_or(c.max_time),
                    Some(7) => c.min_time = float.unwrap_or(c.min_time),
                    Some(8) => c.target_pct = float.unwrap_or(c.target_pct),
                    _ => {}
                }
            } else {
                return Some(Ok(zero));
            }
            if let (Some(s), Some(t @ 0..=2), Some(code)) = (shot, t, code) {
                g.other.jg5.shot_codes.insert((s, t), code);
            }
            yes(done)
        }
        // ---- body parts ----
        // `bpd part trait`: 1 the part node (BPNN), 2 the VATS target (BPNT), 3 the IK
        // start node (BPNI), 4 the part name (BPTN), 5 the gore target bone (NAM4); "" for
        // another trait or a part the data lacks [documented: GetBodyPartTraitString's
        // tables; the subrecords: schema combat.json].
        "getbodyparttraitstring" => {
            // Through `Game::body_part_text`, so SetBodyPartName's change reads back
            // (`crate::nvse_jip8`).
            let sub = match index(args, 2) {
                Some(1) => Some("BPNN"),
                Some(2) => Some("BPNT"),
                Some(3) => Some("BPNI"),
                Some(4) => Some("BPTN"),
                Some(5) => Some("NAM4"),
                _ => None,
            };
            let text = of_sig(g, form(args, 0), b"BPTD").zip(index(args, 1)).zip(sub).and_then(|((b, p), s)| g.body_part_text(b, p, s));
            XVal::Str(text.unwrap_or_default())
        }
        // ---- time ----
        // `[unit]`: the time since the executable started, 0 milliseconds, 1 seconds,
        // 2 minutes [documented: GetTimePlayed; observed: jg5, ratios 1000 and 60]; no unit
        // reads milliseconds, another unit 0 [observed: jg5, unit 3]. NVForever counts from
        // its first game (`mark_launch`).
        "gettimeplayed" => {
            let secs = LAUNCH.get().map_or(0.0, |t| t.elapsed().as_secs_f64());
            XVal::Num(match args.first().map(|_| index(args, 0)) {
                None | Some(Some(0)) => secs * 1000.0,
                Some(Some(1)) => secs,
                Some(Some(2)) => secs / 60.0,
                Some(_) => 0.0,
            })
        }
        // `r g b`: the colour packed as R × 65536 + G × 256 + B [documented: GetRGBColor;
        // observed: jg5, 255 128 1 reads 16744449, 1 0 0 65536, 0 0 1 1]; any channel
        // outside 0..=255 returns 0 [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9
        // + JohnnyGuitarNVSE 528, clean-room script probe jg_16_rgb_color_out_of_range:
        // (256,0,0), (0,256,0), (-1,0,0) -> 0,0,0; no DLL or source consulted].
        "getrgbcolor" => {
            let (r, g, b) = (num(args, 0, 0.0).trunc(), num(args, 1, 0.0).trunc(), num(args, 2, 0.0).trunc());
            let ok = |c: f64| c.is_finite() && (0.0..=255.0).contains(&c);
            XVal::Num(if ok(r) && ok(g) && ok(b) { r * 65536.0 + g * 256.0 + b } else { 0.0 })
        }
        _ => return None,
    };
    let _ = ctx;
    Some(Ok(v))
}
