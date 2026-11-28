//! JohnnyGuitar NVSE: the NIF, particle and camera functions (#1007;
//! docs/spec-notes/ext-jg-nif-camera.md). Distances to drawn nodes and their bounds, a
//! particle system's values, a shape's alpha and stencil properties, switch nodes, the
//! sequences playing, the 3D refresh, and the player's camera: its place, its overrides
//! and where a point of the world falls on the screen [documented: each
//! page in the wiki export]. The nodes are the scene mirror's (`crate::nif_blocks`), the
//! camera what the engine tells each frame (`CameraRequests::view`, `nvf_play::script_camera`).

use crate::functions::camera::CameraView;
use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;

/// Engine-owned runtime reference FormIDs for JohnnyGuitar NVSE's built-in 3D camera
/// overlay references (`JG_CameraOverlayRef_PreIS` and `JG_CameraOverlayRef_PreUI`),
/// reserved in `[CAMERA_OVERLAY_BASE, LAMBDA_BASE)` (`0xFE7F_0000..0xFE80_0000`),
/// disjoint from `CLONE_BASE` (`0xFE00_0000..0xFE20_0000`), `COMPILED_BASE`
/// (`0xFE20_0000..0xFE40_0000`), `TEMP_REF_BASE` (`0xFE40_0000..0xFE7F_0000`),
/// `LAMBDA_BASE` / `MADE_PACKAGE_BASE` (`0xFE80_0000..0xFF00_0000`), and
/// `FormId::FIRST_DYNAMIC` (`0xFF00_0000..`) [design; docs/spec-notes/ext-jg-nif-camera.md].
pub const CAMERA_OVERLAY_PRE_IS: FormId = FormId(crate::game::CAMERA_OVERLAY_BASE + 1);
pub const CAMERA_OVERLAY_PRE_UI: FormId = FormId(crate::game::CAMERA_OVERLAY_BASE + 2);

/// Look up a built-in JohnnyGuitar camera overlay reference by EditorID (case-insensitive).
pub fn camera_overlay_by_edid(id: &str) -> Option<FormId> {
    if id.eq_ignore_ascii_case("JG_CameraOverlayRef_PreIS") {
        Some(CAMERA_OVERLAY_PRE_IS)
    } else if id.eq_ignore_ascii_case("JG_CameraOverlayRef_PreUI") {
        Some(CAMERA_OVERLAY_PRE_UI)
    } else {
        None
    }
}

/// Return the EditorID of a built-in JohnnyGuitar camera overlay reference, if `f` is one.
pub fn camera_overlay_edid(f: FormId) -> Option<&'static str> {
    match f {
        CAMERA_OVERLAY_PRE_IS => Some("JG_CameraOverlayRef_PreIS"),
        CAMERA_OVERLAY_PRE_UI => Some("JG_CameraOverlayRef_PreUI"),
        _ => None,
    }
}

/// Whether `f` is one of JohnnyGuitar's built-in camera overlay references.
pub fn is_camera_overlay_ref(f: FormId) -> bool {
    camera_overlay_edid(f).is_some()
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

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// A result and the values the VM writes into the script's variables (`out_params`).
fn outputs(g: &mut Game, ret: f64, vals: &[f64]) -> XVal {
    list(g, std::iter::once(ret).chain(vals.iter().copied()).map(XVal::Num).collect())
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f64 {
    let d: f32 = (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum();
    f64::from(d.sqrt())
}

/// The camera's right, ahead and up in game space for its heading (clockwise from north,
/// +Y) and pitch (up positive) [design: the engine's convention, `nvf_play::space`].
fn camera_axes(v: &CameraView) -> [[f32; 3]; 3] {
    let (sh, ch) = v.heading.sin_cos();
    let (sp, cp) = v.pitch.sin_cos();
    let ahead = [sh * cp, ch * cp, sp];
    let right = [ch, -sh, 0.0];
    let up = [-sh * sp, -ch * sp, cp];
    [right, ahead, up]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Default D3D9 camera clip planes for `WorldToScreen` NDC `zOut`
/// [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528,
/// 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), `w6_t1_10d_jg_w2s_depth`
/// (`tools/capture/nvse/catalogs/wave6_t1_jg_camera_w2s.json`): `zOut` matches
/// `(5000 / (5000 - 5)) * (1 - 5 / d)` to 5 decimal places across `d = 50, 100, 200,
/// 1000, -100, -200` (`0.90090, 0.95095, 0.97598, 0.99600, 1.05105, 1.02603`);
/// hypothesis: exterior cells with larger far clip planes use the camera's far plane].
const W2S_NEAR: f64 = 5.0;
const W2S_FAR: f64 = 5_000.0;

/// A world point on the screen: X and Y from 0 to 1 (left to right, top to bottom),
/// D3D9 NDC depth `(far / (far - near)) * (1 - near / z)` (`< 1` in front of the
/// camera, `> 1` behind it), and whether it is on the screen. Off the screen, handle
/// type 0 puts it on the edge of the square, 1 on the circle round the centre, 2 leaves it
/// where the projection put it [documented: WorldToScreen's table; observed:
/// `w6_t1_10b_jg_w2s_paren` and `w6_t1_10d_jg_w2s_depth`,
/// `docs/spec-notes/ext-jg-nif-camera.md` §2].
pub fn world_to_screen(v: &CameraView, p: [f32; 3], handle: i64) -> (f64, f64, f64, bool) {
    let [right, ahead, up] = camera_axes(v);
    let rel = [p[0] - v.pos[0], p[1] - v.pos[1], p[2] - v.pos[2]];
    let z = dot(rel, ahead);
    let ty = (v.fov_y / 2.0).tan().max(1e-4);
    let tx = ty * v.aspect.max(1e-4);
    let depth = z.abs().max(1e-4);
    let (mut nx, mut ny) = (dot(rel, right) / (depth * tx), dot(rel, up) / (depth * ty));
    let on = z > 0.0 && nx.abs() <= 1.0 && ny.abs() <= 1.0;
    if !on {
        match handle {
            0 => {
                let m = nx.abs().max(ny.abs()).max(1e-6);
                (nx, ny) = (nx / m, ny / m);
            }
            1 => {
                let m = (nx * nx + ny * ny).sqrt().max(1e-6);
                (nx, ny) = (nx / m, ny / m);
            }
            _ => {}
        }
    }
    let z_signed = if z >= 0.0 { f64::from(z).max(1e-4) } else { f64::from(z).min(-1e-4) };
    let z_ndc = (W2S_FAR / (W2S_FAR - W2S_NEAR)) * (1.0 - W2S_NEAR / z_signed);
    (f64::from((nx + 1.0) / 2.0), f64::from((1.0 - ny) / 2.0), z_ndc, on)
}

impl Game<'_> {
    /// The engine tells where a hit on `target` struck, in the world (a shot's or a blow's
    /// point of impact), for Get3DDistanceFromHitToNiNode.
    pub fn note_hit_point(&mut self, target: FormId, at: [f32; 3]) {
        self.extra.entry(target).or_default().hit_point = Some(at);
    }
}

/// The anim group code of a name (`Game::anim_group_name`'s table read back).
fn group_code_of(g: &Game, name: &str) -> Option<u16> {
    (0..=600u16).find(|&c| g.anim_group_name(c).is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let on = subject.or(ctx.self_ref);
    let v = match name {
        // ---- distances to drawn nodes, in the world [documented: the pages]; 0 without
        // the node or its reference's 3D (see docs/spec-notes/ext-jg-nif-camera.md §1).
        "get3ddistancebetweenninodes" => {
            let (Some(a), Some(b)) = (form(args, 0), form(args, 1)) else { return Some(Ok(zero)) };
            match (crate::nif_blocks::node_world(g, a, &text(args, 2)), crate::nif_blocks::node_world(g, b, &text(args, 3))) {
                (Some(p), Some(q)) => XVal::Num(dist(p, q)),
                _ => zero,
            }
        }
        "get3ddistancetoninode" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let at = [num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32, num(args, 3, 0.0) as f32];
            crate::nif_blocks::node_world(g, r, &text(args, 0)).map_or(zero, |p| XVal::Num(dist(p, at)))
        }
        // From where the last hit on the calling reference struck, as the engine tells it
        // (`RefExtra::hit_point`) [documented: in an OnHit block or the hit's event]; 0
        // without one (see docs/spec-notes/ext-jg-nif-camera.md §1).
        "get3ddistancefromhittoninode" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let Some(hit) = g.extra.get(&r).and_then(|e| e.hit_point) else { return Some(Ok(zero)) };
            crate::nif_blocks::node_world(g, r, &text(args, 0)).map_or(zero, |p| XVal::Num(dist(p, hit)))
        }
        // `[block] [pc]`: the bounding sphere in the world, [x, y, z, radius]; the
        // reference's root without a block [documented]. A missing block name returns the
        // exact 4-element zero bound `[0.0, 0.0, 0.0, 0.0]` [observed: Fallout: New Vegas
        // 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), 10 official base masters, `w4c_b3_11b_jgnif_nodes_bounds`
        // (`bndRoot=4(r=72.1),bndMiss=4(r=0.0)`) and
        // `w6t2_23_jg_dds_texture_format_and_bounds`
        // (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
        // `bndMiss(sz=4,0.0,0.0,0.0,0.0)`; hypothesis: empty array before 3D tree].
        "getnibound" => {
            let Some(r) = on else { return Some(Ok(XVal::Array(0))) };
            let block = text(args, 0);
            fn bound(g: &mut Game, r: FormId, n: &str) -> Option<[f32; 4]> {
                crate::nif_blocks::node_and_children(g, r, n).and_then(|(n, _)| n.info.and_then(|i| i.bound))
            }
            let b = if block.is_empty() {
                crate::nif_blocks::root_name(g, r).and_then(|n| bound(g, r, &n))
            } else if let Some(b) = bound(g, r, &block) {
                Some(b)
            } else if crate::nif_blocks::root_name(g, r).is_some() {
                Some([0.0; 4])
            } else {
                None
            };
            match b {
                Some(b) => list(g, b.iter().map(|x| XVal::Num(f64::from(*x))).collect()),
                None => list(g, Vec::new()),
            }
        }
        // ---- particle systems: the values by the pages' IDs 0-19, the spawn rate apart
        // [documented]. Of the system block (or its emitter object) of that name; 0 / false
        // without one [hypothesis].
        "getnipsysmodifiervalue" | "getparticleemitterspawnrate" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let id = if name == "getparticleemitterspawnrate" { 20.0 } else { num(args, 1, -1.0) };
            let e = crate::nif_blocks::node_and_children(g, r, &text(args, 0)).and_then(|(n, _)| n.info.and_then(|i| i.emitter));
            match (e, usize::try_from(id as i64).ok().filter(|&k| k <= 20)) {
                (Some(e), Some(k)) if name == "getparticleemitterspawnrate" || k < 20 => XVal::Num(f64::from(e[k])),
                _ => zero,
            }
        }
        "setnipsysmodifiervalue" | "setparticleemitterspawnrate" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let (id, v) = if name == "setparticleemitterspawnrate" { (20.0, num(args, 1, f64::NAN)) } else { (num(args, 1, -1.0), num(args, 2, f64::NAN)) };
            let ok = match usize::try_from(id as i64).ok().filter(|&k| k <= 20 && (k < 20 || name == "setparticleemitterspawnrate")) {
                Some(k) if v.is_finite() => g.scene.set_particle(r, &text(args, 0), k, v as f32),
                _ => false,
            };
            XVal::Num(f64::from(u8::from(ok)))
        }
        // ---- a shape's alpha and stencil properties: `block item [pc]`, item 0 the flags,
        // 1 the threshold; 0 the flags, 1 the reference, 2 the mask (see
        // docs/spec-notes/ext-jg-nif-camera.md §1). The setters keep the value for the session's reads and queue
        // `SceneEdit::AlphaProperty` / `SceneEdit::StencilProperty` for the engine to apply
        // live to the drawn shape's material (`nvforever::nif_mirror`). 0 without one.
        "getalphapropertyvalue" | "getstencilpropertyvalue" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let item = num(args, 1, -1.0);
            let info = crate::nif_blocks::node_and_children(g, r, &text(args, 0)).and_then(|(n, _)| n.info);
            let vals: Option<Vec<f32>> = info.and_then(|i| if name == "getalphapropertyvalue" { i.alpha.map(|a| a.to_vec()) } else { i.stencil.map(|s| s.to_vec()) });
            XVal::Num(vals.and_then(|v| usize::try_from(item as i64).ok().and_then(|k| v.get(k).copied())).map_or(0.0, f64::from))
        }
        "setalphapropertyvalue" | "setstencilpropertyvalue" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let (item, v) = (num(args, 1, -1.0), num(args, 2, f64::NAN));
            let alpha = name == "setalphapropertyvalue";
            let ok = v.is_finite() && usize::try_from(item as i64).is_ok_and(|k| g.scene.set_prop(r, &text(args, 0), alpha, k, v as f32));
            XVal::Num(f64::from(u8::from(ok)))
        }
        // `block index [pc]`: the NiSwitchNode shows its child of that index alone
        // [documented: "Sets the active index"]; the others are hidden by their flag
        // (bit 0), each child by its place under the switch, not by its name: a switch's
        // children are often unnamed or share names (#2017). An index past the children
        // hides them all (see docs/spec-notes/ext-jg-nif-camera.md §1).
        "setswitchnodeindex" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let k = num(args, 1, -1.0) as i64;
            crate::nif_blocks::show_one_child(g, r, &text(args, 0), usize::try_from(k).ok());
            zero
        }
        // `sequence [block] [pc]`: a sequence of that name playing on the reference: its
        // body's clip or a sequence a script played (kNVSE's list, `Game::playing_sequences`),
        // by file name without the folder and extension, or its movement group
        // (see docs/spec-notes/ext-jg-nif-camera.md §1).
        "isnisequenceactive" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            let want = text(args, 0);
            let stem = |p: &str| p.rsplit(['\\', '/']).next().unwrap_or(p).to_string();
            let stem = |p: &str| { let f = stem(p); match f.len().checked_sub(3) { Some(k) if f.is_char_boundary(k) && f[k..].eq_ignore_ascii_case(".kf") => f[..k].to_string(), _ => f } };
            let playing = g.playing_sequences(r).iter().any(|p| stem(p).eq_ignore_ascii_case(&want) || p.eq_ignore_ascii_case(&want));
            let group = g.senses.anim.get(&r).and_then(|s| s.group).is_some_and(|gr| gr.eq_ignore_ascii_case(&want));
            XVal::Num(f64::from(u8::from(playing || group)))
        }
        // `slot`: the anim group playing in that sequence slot, 0 idle … 7 special idle
        // [documented]. Slot 0 holds Idle on a standing actor [observed: jgnif.sequences,
        // 0 on the player]; the body's other movement groups fill slot 1 (movement). Slot 4
        // (weapon/hand actions) holds 24 (Equip) when a weapon is drawn or equipping, and
        // 200 (`JamA`, + `DNAM/reload_animation` [hypothesis]) during `AnimAction::Jam`
        // [observed 2026-11-24: `w6t2_14_jip_on_reload_and_seq_anim_group`:
        // `s0=0,s4(pre=24,t0=200,t1=200,cb=200)`]; standing holstered returns -1
        // [observed: jgnif.sequences].
        "getsequenceanimgroup" => {
            let Some(r) = on else { return Some(Ok(XVal::Num(-1.0))) };
            let slot = num(args, 0, -1.0) as i64;
            let code = if slot == 4 {
                let action = g.senses.anim.get(&r).and_then(|s| s.action);
                match action {
                    Some(crate::anim_state::AnimAction::Jam) => {
                        let w = crate::functions::items::equipped_weapon(g, r)
                            .or_else(|| g.extra.get(&r).and_then(|e| e.held_weapon));
                        let off = w
                            .and_then(|w| crate::nvse::record_value(g, w, "DNAM", "reload_animation"))
                            .and_then(|v| v.num().ok())
                            .filter(|n| n.is_finite() && (0.0..=22.0).contains(n))
                            .map_or(0u16, |n| n.trunc() as u16);
                        Some(200 + off)
                    }
                    Some(crate::anim_state::AnimAction::Equip) => Some(24),
                    Some(crate::anim_state::AnimAction::Unequip) => Some(25),
                    Some(crate::anim_state::AnimAction::Stagger) => Some(223),
                    None if g.extra.get(&r).is_some_and(|e| e.weapon_out) => Some(24),
                    _ => None,
                }
            } else {
                let group = g.senses.anim.get(&r).and_then(|s| s.group).unwrap_or("Idle");
                let idle = group.eq_ignore_ascii_case("Idle");
                ((slot == 0 && idle) || (slot == 1 && !idle)).then_some(group).and_then(|gr| group_code_of(g, gr))
            };
            XVal::Num(code.map_or(-1.0, f64::from))
        }
        // The engine updates every drawn node each frame, so there is nothing to bring up
        // to date [decision]. 1 on a reference [observed: jgnif.refresh], 0 without one
        // [hypothesis].
        "updatescenegraph" => XVal::Num(f64::from(u8::from(on.is_some()))),
        // `flags`: 1 the model, 2 the skin, 4 the head, 8 the face drawn again (an NPC's
        // through the appearance changes the engine draws, `crate::appearance`); 16 scale,
        // 32 lights and 64 position the engine takes each frame [documented: the flag
        // table; decision: those need no request].
        "update3dalt" => {
            let Some(r) = on else { return Some(Ok(zero)) };
            if num(args, 0, 0.0) as i64 & 0xF != 0 {
                g.appearance_changes.push(crate::appearance::AppearanceChange { actor: r, reset: false });
            }
            // 1 [observed: jgnif.refresh, flags 32 on the player].
            XVal::Num(1.0)
        }
        // ---- the camera
        // `x y z worldOrLocal` (the VM writes x, y, z): the camera's place in the world
        // (`0`), or its local translation (`1`): `(0, 0, 0)` in first person [observed:
        // `w6_t1_10a_jg_cam_trans`, `docs/spec-notes/ext-jg-nif-camera.md` §2], and in
        // third person relative to the player in the player's frame, x right, y ahead, z up
        // [documented: "local or world camera position", the example's 1 local;
        // hypothesis: third-person local frame]. 0 0 0 without an engine.
        "getcameratranslation" => {
            let local = num(args, 0, 0.0) != 0.0;
            let Some(v) = g.ui.camera.view else { return Some(Ok(outputs(g, 0.0, &[0.0, 0.0, 0.0]))) };
            let p = if local {
                if v.first_person {
                    [0.0; 3]
                } else {
                    let (pp, pr) = g.ref_pos_rot(FormId::PLAYER_REF).unwrap_or(([0.0; 3], [0.0; 3]));
                    let d = [v.pos[0] - pp[0], v.pos[1] - pp[1], v.pos[2] - pp[2]];
                    let (s, c) = pr[2].sin_cos();
                    [d[0] * c - d[1] * s, d[0] * s + d[1] * c, d[2]]
                }
            } else {
                v.pos
            };
            outputs(g, 0.0, &p.map(f64::from))
        }
        // `override x y z`: the third-person camera's place relative to the player (as
        // GetCameraTranslation 1), or the override cleared with 0 [documented]; the
        // engine puts the camera there (`nvf_play::script_camera`).
        "setcameratranslate" => {
            g.ui.camera.translate_override = (num(args, 0, 0.0) != 0.0).then(|| [num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32, num(args, 3, 0.0) as f32]);
            zero
        }
        // `override axis angle [ref]`: the third-person camera's angle about axis 1 X, 2
        // Y, 3 Z in degrees, or cleared; with a reference the camera keeps looking at it
        // [documented: "Optionally tracks a reference"]. Clearing an axis clears the
        // tracking (see docs/spec-notes/ext-jg-nif-camera.md §2).
        "setcamerarotate" => {
            let axis = num(args, 1, 0.0) as i64;
            if let Ok(k @ 0..=2) = usize::try_from(axis - 1) {
                let cam = &mut g.ui.camera;
                if num(args, 0, 0.0) != 0.0 {
                    cam.rotate_override[k] = Some(num(args, 2, 0.0) as f32);
                    if let Some(t) = form(args, 3) {
                        cam.rotate_track = Some(t);
                    }
                } else {
                    cam.rotate_override[k] = None;
                    cam.rotate_track = None;
                }
            }
            zero
        }
        // `xOut yOut zOut x y z handleType [ref]` (the VM writes the three): where the point
        // falls on the screen (`world_to_screen`); with a reference the point is an offset
        // from its place [documented]. 0 and the variables 0 without an engine.
        "worldtoscreen" => {
            let mut p = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            if let Some((rp, _)) = form(args, 4).and_then(|r| g.ref_pos_rot(r)) {
                p = [p[0] + rp[0], p[1] + rp[1], p[2] + rp[2]];
            }
            let Some(v) = g.ui.camera.view else { return Some(Ok(outputs(g, 0.0, &[0.0, 0.0, 0.0]))) };
            let (x, y, z, on) = world_to_screen(&v, p, num(args, 3, 0.0) as i64);
            outputs(g, f64::from(u8::from(on)), &[x, y, z])
        }
        _ => return None,
    };
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> CameraView {
        CameraView { pos: [0.0; 3], heading: 0.0, pitch: 0.0, roll: 0.0, fov_y: std::f32::consts::FRAC_PI_2, aspect: 1.0, first_person: true }
    }

    #[test]
    fn points_on_the_screen() {
        let v = view();
        // Straight ahead: centre (0.5, 0.5) and D3D9 NDC depth across observed distances
        // [observed: w6_t1_10d_jg_w2s_depth].
        for (d, want_z, want_on) in [
            (50.0, 0.90090, true),
            (100.0, 0.95095, true),
            (200.0, 0.97598, true),
            (1000.0, 0.99600, true),
            (-100.0, 1.05105, false),
            (-200.0, 1.02603, false),
        ] {
            let (x, y, z, on) = world_to_screen(&v, [0.0, d, 0.0], 2);
            assert!((x - 0.5).abs() < 1e-6 && (y - 0.5).abs() < 1e-6 && (z - want_z).abs() < 1e-5 && on == want_on, "d={d}: got ({x}, {y}, {z}, {on})");
        }
        // At 45° right and up with a 90° view: the corner.
        let (x, y, _, on) = world_to_screen(&v, [100.0, 100.0, 100.0], 0);
        assert!((x - 1.0).abs() < 1e-5 && y.abs() < 1e-5 && on);
        // Far right, off the screen: on the square's edge, or where it falls.
        let (x, _, _, on) = world_to_screen(&v, [300.0, 100.0, 0.0], 0);
        assert!(!on && (x - 1.0).abs() < 1e-5);
        let (x, _, _, _) = world_to_screen(&v, [300.0, 100.0, 0.0], 2);
        assert!((x - 2.0).abs() < 1e-5);
        // Facing east (heading 90°), a point east is ahead.
        let east = CameraView { heading: std::f32::consts::FRAC_PI_2, ..view() };
        assert!(world_to_screen(&east, [100.0, 0.0, 0.0], 0).3);
    }
}
