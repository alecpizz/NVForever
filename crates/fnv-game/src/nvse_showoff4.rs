//! ShowOff NVSE functions of batch 4 (#1010): the matrix and quaternion math on the VM's
//! arrays, SayTo's topic, hotkey icons, the pickpocket and quest-message switches, the
//! corner-message handler, frozen ammo regeneration, ResetInteriorAlt, SetOwnershipTemp,
//! the enable-parent functions and ClearShowoffSavedData [documented: each page in the
//! wiki export; docs/spec-notes/ext-showoff.md §7]. The INI cache, JSON and folder
//! functions of the batch are in `crate::mod_files`. Changes last for the session.

use crate::game::{Game, RunCtx};
use crate::trace::EventKind;
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

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

// ---- matrices -------------------------------------------------------------------------

impl Game<'_> {
    /// Live actor weapon spread in spread units (`GetCalculatedActorSpread`)
    /// [documented: GECK wiki Gun Spread Formula (New Vegas), Perk Entry Points;
    /// observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 +
    /// ShowOffNVSE 184, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official
    /// base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe;
    /// zero writes to game install or Saves), `w4c_a06b_spread_settled` (`#3679`) and
    /// settled 6-type `w4d_09_spread_all_anim_types`
    /// (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3802 / PR #3803):
    /// `PROBE|w4d_09_spread_all_anim_types|t3(0.1434,0.2359,0.1434,0.3284)|t4(0.1388,0.2313,0.1388,0.3238)|t5(0.1688,0.3538,0.3538,0.4463)|t6(0.1434,0.3284,0.3284,0.4209)|t8(0.1804,0.3654,0.3654,0.4579)|t10(0.4504,0.4459,0.4504,0.4459)|t3nw(0.1434,0.1434)`
    /// (`t3nw(0.1434,0.1434)` confirms `SetNoGunWobble 1` leaves settled spread
    /// unchanged; 1H guns anim 3/4: right +0.2, left 0, both +0.4; 2H weapons anim
    /// 5/6/8: either single arm +0.4, both arms +0.6; thrown anim 10: 0 for
    /// right/left/both); zero DLL, PDB, or third-party source consulted; hypothesis:
    /// unprobed anim types 0..=2, 7, 9, 11..=13 and actor-spread component after
    /// `CALCULATE_GUN_SPREAD` perks, excluding weapon `min_spread` and ammo spread mods].
    pub fn calculated_actor_spread(&mut self, actor: FormId) -> f64 {
        let Some(w) = crate::functions::items::equipped_weapon(self, actor) else {
            return 0.0;
        };
        let base = crate::functions::base_of(self, w).unwrap_or(w);
        let field = |g: &Game, path: &str| match crate::nvse::record_value(g, base, "DNAM", path) {
            Some(XVal::Num(x)) => x,
            _ => 0.0,
        };
        let setting = |g: &Game, name: &str, d: f64| g.world.setting_f32(name).map_or(d, f64::from);

        let aiming = actor == FormId::PLAYER_REF && (self.jip2.told.aiming || self.jip2.told.sights_up);
        let m = self.motion(actor);
        let unaimed = if aiming { 0.0 } else { setting(self, "fUnaimedSpreadPenalty", 0.2) };
        let (aim_move_div, _) = self.entry_point_with(actor, crate::perks::MODIFY_AIMING_MOVE_SPEED, &[Some(base)], 1.0);
        let moving = if !m.moving {
            0.0
        } else if m.running {
            setting(self, "fRunningSpreadPenalty", 0.2)
        } else {
            setting(self, "fWalkingSpreadPenalty", 0.1)
        } / aim_move_div.max(1.0);
        let standing = if m.sneaking { 0.0 } else { setting(self, "fStandingSpreadPenalty", 0.1) };

        let left_arm = self.is_crippled(actor, crate::combat::LEFT_ARM);
        let right_arm = self.is_crippled(actor, crate::combat::RIGHT_ARM);
        // Crippled-arm spread penalty by `WEAP.DNAM.animation_type`
        // [observed: `w4d_09_spread_all_anim_types` in `wave4d_extenders.json` (#3802 / PR #3803)
        // and `w6t2p2_07_spread_anim_7_9_2_1_0` in `wave6_t2_observed_matches.json` (2026-11-24:
        // `t8(0.6212,0.4232,0.4232,0.5157)|t9(0.1781,0.3631,0.3631,0.4556)|t2(1.0000,1.0000)`)]:
        // - 1H guns (anim 3, 4): right arm +0.2, left arm alone 0, both arms +0.4.
        // - 2H ranged (anim 5, 6, 8, 9 observed; 7 grouped as [hypothesis]): either single
        //   arm +0.4, both arms +0.6 (`fCrippledArms2HSpreadPenalty`).
        // - Melee/thrown/grenade/mine/unarmed (anim 2 and 10 observed +0.0; 0..=1 and 11..=13
        //   grouped as [hypothesis]): 0.0 for right, left, and both arms.
        let anim = field(self, "animation_type") as i64;
        let arms = match anim {
            3 | 4 => match (left_arm, right_arm) {
                (true, true) => setting(self, "fCrippledArms1HSpreadPenalty", 0.4),
                (_, true) => setting(self, "fCrippledArm1HSpreadPenalty", 0.2),
                _ => 0.0,
            },
            5..=9 => match (left_arm, right_arm) {
                (true, true) => setting(self, "fCrippledArms2HSpreadPenalty", 0.6),
                (true, false) | (false, true) => setting(self, "fCrippledArm2HSpreadPenalty", 0.4),
                (false, false) => 0.0,
            },
            _ => 0.0,
        };

        let skill_code = field(self, "skill") as u16;
        let skill = crate::functions::actor::actor_value(self, actor, skill_code).unwrap_or(0.0);
        let strength = crate::functions::actor::actor_value(self, actor, crate::actor_values::STRENGTH).unwrap_or(0.0);
        let (str_req_cut, _) = self.entry_point_with(actor, crate::perks::MODIFY_WEAPON_STRENGTH_REQ, &[Some(base)], 0.0);
        let str_req = (field(self, "strength_req") - str_req_cut - strength.min(10.0)).max(0.0)
            * setting(self, "fWeapStrengthReqPenalty", 0.025);
        let skill_req = (field(self, "skill_req") - skill.min(100.0)).max(0.0)
            * setting(self, "fWeapSkillReqPenalty", 0.01);
        let skill_mult = 1.0 - setting(self, "fWobbleToSkillConversion", 0.5) * skill.clamp(0.0, 100.0) / 100.0;
        let raw = (skill_mult * (unaimed + moving + standing + arms + str_req + skill_req))
            .max(setting(self, "fMinGunSpreadValue", 0.01));

        let (spread, _) = self.entry_point_with(actor, crate::perks::CALCULATE_GUN_SPREAD, &[Some(base)], raw);
        spread.max(0.0)
    }
}

/// A matrix read from an array: its rows, and whether the array was one-dimensional (a
/// single row of numbers).
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    pub rows: Vec<Vec<f64>>,
    pub flat: bool,
}

impl Matrix {
    fn cols(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }
}

/// An array as a matrix: a list of numbers (one row), or a list of lists of numbers all
/// the same length; empty arrays, mixed ones, other values and ragged rows are no matrix
/// [documented: Matrix_IsMatrix and its examples]. Only list arrays count [observed:
/// `so_01_mat_is_stringmap` (`wave4a_extenders.json`), see docs/spec-notes/ext-showoff.md §7].
pub fn read_matrix(g: &Game, v: &XVal) -> Option<Matrix> {
    let XVal::Array(id) = v else { return None };
    let a = g.ext.array(*id).filter(|a| a.kind == ArrayKind::List && !a.entries.is_empty())?;
    let nums = |vals: &[(XKey, XVal)]| -> Option<Vec<f64>> {
        vals.iter()
            .map(|(_, x)| match x {
                XVal::Num(n) => Some(*n),
                _ => None,
            })
            .collect()
    };
    if let Some(row) = nums(&a.entries) {
        return Some(Matrix { rows: vec![row], flat: true });
    }
    let mut rows = Vec::new();
    for (_, x) in &a.entries {
        let XVal::Array(r) = x else { return None };
        let r = g.ext.array(*r).filter(|r| r.kind == ArrayKind::List && !r.entries.is_empty())?;
        rows.push(nums(&r.entries)?);
    }
    let n = rows[0].len();
    rows.iter().all(|r| r.len() == n).then_some(Matrix { rows, flat: false })
}

/// A matrix back as an array: one-dimensional when it is a single row made from a
/// one-dimensional input (observed for scalar operations and 1-D transpose in
/// `so_04_mat_transpose_1d_shape` / `w4c_b6_08_matrix_div0_and_1d_trans`; see
/// docs/spec-notes/ext-showoff.md §7 for remaining matrix-op shape `[hypothesis]`).
fn write_matrix(g: &mut Game, m: Matrix) -> XVal {
    if m.flat && m.rows.len() == 1 {
        let row = m.rows.into_iter().next().unwrap_or_default();
        return list(g, row.into_iter().map(XVal::Num).collect());
    }
    let rows: Vec<XVal> = m.rows.into_iter().map(|r| list(g, r.into_iter().map(XVal::Num).collect())).collect();
    list(g, rows)
}

/// The element-wise operations and the product [documented:
/// Matrix_ApplyOperationWithMatrix's table: `+`, `-`, `/` and `%` element by element
/// (the Schur product), `*` the matrix product, defined when A's columns are B's rows].
/// `None` for another operator or shapes that do not fit.
pub fn matrix_op(a: &Matrix, op: &str, b: &Matrix) -> Option<Matrix> {
    if op == "*" {
        if a.cols() != b.rows.len() {
            return None;
        }
        let rows = a.rows.iter().map(|r| (0..b.cols()).map(|j| r.iter().zip(&b.rows).map(|(x, br)| x * br[j]).sum()).collect()).collect();
        return Some(Matrix { rows, flat: a.flat });
    }
    let f: fn(f64, f64) -> f64 = match op {
        "+" => |x, y| x + y,
        "-" => |x, y| x - y,
        "/" => |x, y| x / y,
        "%" => |x, y| x * y,
        _ => return None,
    };
    if a.rows.len() != b.rows.len() || a.cols() != b.cols() {
        return None;
    }
    let rows = a.rows.iter().zip(&b.rows).map(|(r, s)| r.iter().zip(s).map(|(x, y)| f(*x, *y)).collect()).collect();
    Some(Matrix { rows, flat: a.flat })
}

/// Each element with a scalar: `+`, `-`, `*`, `/` [documented:
/// Matrix_ApplyOperationWithScalar; "%" is not supported]. Division by 0 returns a
/// same-length array of floating-point infinities (`inf`) rather than `ar_Null`
/// [observed: `so_02_mat_scalar_div0` (`wave4a_extenders.json`),
/// `w4c_b6_08_matrix_div0_and_1d_trans` (`wave4c_extenders.json`), and
/// `w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags`
/// (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`: `div0=3` on a
/// 3-element row), see docs/spec-notes/ext-showoff.md §7].
pub fn scalar_op(a: &Matrix, op: &str, s: f64) -> Option<Matrix> {
    let f: fn(f64, f64) -> f64 = match op {
        "+" => |x, y| x + y,
        "-" => |x, y| x - y,
        "*" => |x, y| x * y,
        "/" => |x, y| x / y,
        _ => return None,
    };
    Some(Matrix { rows: a.rows.iter().map(|r| r.iter().map(|x| f(*x, s)).collect()).collect(), flat: a.flat })
}

/// The transpose [documented: Matrix_Transpose's example]; a one-dimensional row becomes
/// a column (a list of one-element lists) [observed 2026-11-24:
/// `w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags`
/// (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `tr(sz=3,sub=1)` on
/// `ar_List 1, 2, 3`].
pub fn transpose(a: &Matrix) -> Matrix {
    let rows = (0..a.cols()).map(|j| a.rows.iter().map(|r| r[j]).collect()).collect();
    Matrix { rows, flat: false }
}

/// The rotation matrix of a quaternion `[w, x, y, z]` [documented: Quaternion_GetMatrix's
/// layout]. Not normalised: the diagonal is `2(w² + a²) − 1` [observed 2026-11-08:
/// showoff4.quat.unnormal, (2 0 0 0) gives 7 7 7], and the matrix is the transpose of the
/// usual right-handed R·v form [observed: showoff4.quat.z90, (0.7071 0 0 0.7071) gives
/// m[0][1] 1 and m[1][0] −1; x and y 90° rotations transpose alike in
/// `so_03_quat_getmat_xy_transpose` (`wave4a_extenders.json`), see
/// docs/spec-notes/ext-showoff.md §7], worked in
/// single precision [observed: m[0][0] −5.96046e-08, exactly 2·0.70710677² − 1 in f32].
/// A zero quaternion gives −1 on the diagonal [hypothesis: nothing is refused once
/// nothing is normalised].
pub fn quat_to_matrix(q: [f64; 4]) -> Option<[[f64; 3]; 3]> {
    if q.iter().any(|c| !c.is_finite()) {
        return None;
    }
    let [w, x, y, z] = q.map(|c| c as f32);
    let m = [
        [2.0 * (w * w + x * x) - 1.0, 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)],
        [2.0 * (x * y - w * z), 2.0 * (w * w + y * y) - 1.0, 2.0 * (y * z + w * x)],
        [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 2.0 * (w * w + z * z) - 1.0],
    ];
    Some(m.map(|r| r.map(f64::from)))
}

/// The quaternion `[w, x, y, z]` of a rotation matrix in the usual right-handed form (so
/// not the inverse of [`quat_to_matrix`], which gives the transpose [observed 2026-11-08:
/// showoff4.quat.back and quat.back.270 equal with this, quat.z90 transposed]), by the largest of the trace and the diagonal; `w` not negative
/// [observed 2026-11-24: `w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags`
/// (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `diag(-1, -1, 1)`
/// yields `q(sz=4,w=0.00,z=1.00)`; hypothesis: sign convention for other negative-trace
/// rotations].
pub fn matrix_to_quat(m: [[f64; 3]; 3]) -> [f64; 4] {
    let t = m[0][0] + m[1][1] + m[2][2];
    let q = if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        [0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s]
    };
    if q[0] < 0.0 {
        q.map(|c| -c)
    } else {
        q
    }
}

/// Matrix_Dump's console lines: a heading, then each row, four decimals [documented: the
/// page's notation; the heading's words are NVForever's own].
pub fn dump_lines(id: u32, m: Option<&Matrix>) -> Vec<String> {
    let Some(m) = m else { return vec![format!("Array {id} is not a matrix")] };
    let mut out = vec![format!("Matrix of array {id}, {} by {}:", m.rows.len(), m.cols())];
    out.extend(m.rows.iter().map(|r| r.iter().map(|x| format!("{x:>11.4}")).collect::<String>()));
    out
}

// ---- the rest -------------------------------------------------------------------------

impl Game<'_> {
    /// Each frame (`Game::hud_tick`): SetShowOffOnCornerMessageEventHandler's functions
    /// get the corner messages queued since the last frame: the text, the icon type (0),
    /// the icon path, the sound path and the seconds, a string that is empty passed as
    /// "<void>" [documented: SetShowOffOnCornerMessageEventHandler]. The sound path is
    /// always "<void>": NVForever's corner messages carry no sound [hypothesis: the page
    /// says it is "usually null"]. Run on the next frame rather than inside the queueing
    /// call [hypothesis: the page says only "whenever a corner message is appended"].
    pub(crate) fn corner_handlers_run(&mut self) {
        if self.showoff.corner_pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.showoff.corner_pending);
        let handlers = self.showoff.corner_handlers.clone();
        let void = |s: &str| XVal::Str(if s.is_empty() { "<void>".to_string() } else { s.to_string() });
        for (msg, icon, secs) in pending {
            for f in &handlers {
                let args = vec![void(&msg), XVal::Num(0.0), void(&icon), void(""), XVal::Num(secs)];
                if let Err(error) = self.run_function(f, None, args, None, RunCtx::default()) {
                    self.emit(EventKind::ScriptError { unit: "corner message handler".into(), error });
                }
            }
        }
    }

    /// The enable state of `r` after `change` (an Alt function or SetEnableParent), with
    /// the events Enable and Disable send when the state the world sees changed but the
    /// reference's own flag did not.
    fn enable_change(&mut self, r: FormId, change: impl FnOnce(&mut Game) -> Result<(), String>) -> Result<(), String> {
        let before = self.world.is_enabled(r).map_err(|e| e.to_string())?;
        let own_before = self.world.get(r).map_err(|e| e.to_string())?.enabled();
        change(self)?;
        let after = self.world.is_enabled(r).map_err(|e| e.to_string())?;
        let own_after = self.world.get(r).map_err(|e| e.to_string())?.enabled();
        if before != after && own_before == own_after {
            let event = if after { "OnEnable" } else { "OnDisable" };
            self.emit(if after { EventKind::RefEnabled { reference: r, fade: false } } else { EventKind::RefDisabled { reference: r, fade: false } });
            if after {
                self.note_enabled(r);
            }
            self.dispatch_event(event, vec![XVal::Ref(r.0), XVal::Num(0.0)], None);
        }
        Ok(())
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- matrices [documented: each Matrix page; failures give no array] ----
        "matrix_ismatrix" | "mat_ismat" => yes(args.first().and_then(|a| read_matrix(g, a)).is_some()),
        "matrix_transpose" | "mat_t" => match args.first().and_then(|a| read_matrix(g, a)) {
            Some(m) => write_matrix(g, transpose(&m)),
            None => zero,
        },
        "matrix_applyoperationwithmatrix" | "mat_applyopwithmat" => {
            let (a, b) = (args.first().and_then(|a| read_matrix(g, a)), args.get(2).and_then(|b| read_matrix(g, b)));
            match a.zip(b).and_then(|(a, b)| matrix_op(&a, text(args, 1).trim(), &b)) {
                Some(m) => write_matrix(g, m),
                None => zero,
            }
        }
        "matrix_applyoperationwithscalar" | "mat_applyopwithscal" => match args.first().and_then(|a| read_matrix(g, a)).and_then(|a| scalar_op(&a, text(args, 1).trim(), num(args, 2, 0.0))) {
            Some(m) => write_matrix(g, m),
            None => zero,
        },
        "matrix_dump" | "mat_dump" => {
            let id = match args.first() {
                Some(XVal::Array(id)) => *id,
                _ => 0,
            };
            let m = args.first().and_then(|a| read_matrix(g, a));
            g.console_out.extend(dump_lines(id, m.as_ref()));
            zero
        }
        // `[w, x, y, z]`: the 3x3 rotation matrix [documented: Quaternion_GetMatrix].
        "quaternion_getmatrix" | "quat_getmat" => {
            let q = args.first().and_then(|a| read_matrix(g, a)).filter(|m| m.rows.len() == 1 && m.cols() == 4).map(|m| [m.rows[0][0], m.rows[0][1], m.rows[0][2], m.rows[0][3]]);
            match q.and_then(quat_to_matrix) {
                Some(r) => write_matrix(g, Matrix { rows: r.iter().map(|row| row.to_vec()).collect(), flat: false }),
                None => zero,
            }
        }
        // A 3x3 rotation matrix: its `[w, x, y, z]` [documented: Matrix3x3_GetQuaternion].
        "matrix3x3_getquaternion" | "mat_getquat" => {
            let m = args.first().and_then(|a| read_matrix(g, a)).filter(|m| m.rows.len() == 3 && m.cols() == 3);
            match m {
                Some(m) => {
                    let r = [[m.rows[0][0], m.rows[0][1], m.rows[0][2]], [m.rows[1][0], m.rows[1][1], m.rows[1][2]], [m.rows[2][0], m.rows[2][1], m.rows[2][2]]];
                    let q = matrix_to_quat(r);
                    list(g, q.iter().map(|&c| XVal::Num(c)).collect())
                }
                None => zero,
            }
        }
        // ---- SayTo ----
        // The topic and the response the actor's SayTo line says, none when it says none
        // [documented: SayTo_GetTopic, SayTo_GetTopicInfo].
        "sayto_gettopic" | "sayto_gettopicinfo" => {
            let line = subject.and_then(|a| g.speech.iter().find(|s| s.speaker == a && s.say_to));
            XVal::Ref(line.map_or(0, |s| if name == "sayto_gettopic" { s.topic.0 } else { s.info.0 }))
        }
        // ---- hotkey icons ----
        // `item [onlyOverride]`: the override, else the item's icon (GetTexturePath);
        // with onlyOverride 1 the override or "" [documented: GetItemHotkeyIconPath].
        "getitemhotkeyiconpath" => {
            let Some(item) = form(args, 0) else { return Some(Ok(XVal::Str(String::new()))) };
            let over = g.showoff.hotkey_icons.get(&item).cloned();
            let own = || match crate::nvse::record_value(g, item, "ICON", "") {
                Some(XVal::Str(p)) => p,
                _ => String::new(),
            };
            XVal::Str(match over {
                Some(p) => p,
                None if num(args, 1, 0.0) != 0.0 => String::new(),
                None => own(),
            })
        }
        // `item path`: the hotkeys' icon for the item; "" resets it [documented:
        // SetItemHotkeyIconPath]. Read by `Game::item_hotkey_icon_path` for the engine's
        // hotkey pictures (`weapon_wheel::draw`, `pipboy::hotkey_panel`).
        "setitemhotkeyiconpath" => {
            if let Some(item) = form(args, 0) {
                let p = text(args, 1);
                if p.is_empty() {
                    g.showoff.hotkey_icons.remove(&item);
                } else {
                    g.showoff.hotkey_icons.insert(item, p);
                }
            }
            zero
        }
        // ---- switches ----
        // [documented: Get/SetPlayerCanPickpocketEquippedItems, aliases
        // Get/SetPCCanStealEqItems; the pickpocket menu reads it, `Game::pickpocket_worn`].
        "getplayercanpickpocketequippeditems" | "getpccanstealeqitems" => yes(g.pickpocket.worn_items),
        "setplayercanpickpocketequippeditems" | "setpccanstealeqitems" => {
            g.pickpocket.worn_items = num(args, 0, 0.0) != 0.0;
            zero
        }
        // The quest lines waiting are dropped, as KillQuestUpdates [documented:
        // ClearCinematicTextQueue, alias ClearQuestMessageQueue, "Clears the quest message
        // queue"; hypothesis: the line on screen finishes].
        "clearcinematictextqueue" | "clearquestmessagequeue" => {
            g.ui.quest_updates_killed += 1;
            zero
        }
        // `[bOn]`: quest lines on (1) or off (0), 1 when that changed, 0 when unchanged;
        // -1 or nothing reads 1 while they are on and 0 while off [documented:
        // ToggleQuestMessages; observed: `so_07_toggle_quest_messages`
        // (`wave4a_extenders.json`), see docs/spec-notes/ext-showoff.md §7].
        "togglequestmessages" => {
            let b = num(args, 0, -1.0);
            if b == -1.0 {
                yes(!g.ui.quest_messages_off)
            } else {
                let off = b == 0.0;
                let changed = g.ui.quest_messages_off != off;
                g.ui.quest_messages_off = off;
                yes(changed)
            }
        }
        // `setOrRemove handler [flag]` [documented: SetShowOffOnCornerMessageEventHandler,
        // alias SetOnCornerMessageEventHandler; the flag is reserved].
        "setshowoffoncornermessageeventhandler" | "setoncornermessageeventhandler" => {
            let Some(f) = args.get(1).cloned() else { return Some(Ok(zero)) };
            g.showoff.corner_handlers.retain(|h| *h != f);
            if num(args, 0, 1.0) != 0.0 {
                g.showoff.corner_handlers.push(f);
            }
            zero
        }
        // `[toggle]`: whether regeneration was frozen; with an argument it is frozen (1)
        // or runs (0) [documented: FreezeAmmoRegen].
        "freezeammoregen" => {
            let was = g.showoff.ammo_regen_frozen;
            if let Some(XVal::Num(t)) = args.first() {
                g.showoff.ammo_regen_frozen = *t != 0.0;
            }
            yes(was)
        }
        // NVForever never stops loading cells while time is frozen (FreezeTime stops the
        // game's clock only), which is all the patch does [documented: PatchFreezeTime].
        "patchfreezetime" => zero,
        // `cell`: reset at its next load, its dead actors that respawn brought back
        // [documented: ResetInteriorAlt, alias IWantThisRoomCleanRightNow;
        // `crate::respawn`]. An exterior cell is refused (0) as ResetInterior's
        // [hypothesis].
        "resetinterioralt" | "iwantthisroomcleanrightnow" => {
            let Some(cell) = form(args, 0).filter(|&c| g.data.sig(c) == Some(*b"CELL")) else { return Some(Ok(zero)) };
            if crate::functions::fo3::exterior(g, cell) {
                return Some(Ok(zero));
            }
            g.forms.expired_cells_full.insert(cell);
            yes(true)
        }
        // `[newOwner] [cell]`: SetOwnership on the reference, or SetCellOwnership on the
        // cell, for the session only: a save keeps the owner from before [documented:
        // SetOwnershipTemp; `ShowOffState::temp_owners`, `Game::snapshot`]. The owner is
        // the player by default; an actor reference stands for its base.
        "setownershiptemp" => {
            let owner = match form(args, 0) {
                None | Some(FormId::PLAYER_REF) => FormId::PLAYER_BASE,
                Some(f) => crate::functions::base_of(g, f).filter(|b| matches!(g.data.sig(*b).as_ref(), Some(b"NPC_" | b"CREA"))).unwrap_or(f),
            };
            if let Some(cell) = form(args, 1) {
                let before = g.forms.cell_owners.get(&cell).copied();
                g.showoff.temp_owners.entry(cell).or_insert(before);
                g.forms.cell_owners.insert(cell, owner);
                return Some(Ok(yes(true)));
            }
            let Some(r) = subject else { return Some(Ok(zero)) };
            let Ok(st) = g.world.get(r) else { return Some(Ok(zero)) };
            let before = st.extra.owner;
            st.extra.owner = Some(owner);
            g.showoff.temp_owners.entry(r).or_insert(before);
            yes(true)
        }
        // `unused auxTimers`: 1 the calling mod's private permanent timers, 2 every public
        // permanent one, stopped without their stop event [documented:
        // ClearShowoffSavedData]. A timer whose name starts with "*" is temporary and one
        // whose name starts with "_" is public [hypothesis: the aux variables' naming,
        // which the timers' page does not restate].
        "clearshowoffsaveddata" => {
            let which = num(args, 1, 0.0) as i64;
            let me = g.other.plugin.to_ascii_lowercase();
            let mods = g.showoff.timer_mods.clone();
            let gone = |k: &(FormId, String)| -> bool {
                let n = k.1.as_str();
                match which {
                    1 => !n.starts_with('*') && !n.starts_with('_') && mods.get(k).is_some_and(|m| *m == me),
                    2 => n.starts_with('_'),
                    _ => false,
                }
            };
            g.jip2.timers.retain(|k, _| !gone(k));
            g.showoff.countdowns.retain(|k, _| !gone(k));
            g.showoff.timer_mods.retain(|k, _| !gone(k));
            zero
        }
        // ---- enable parents ----
        // `[fade]`: Disable and Enable that work on a reference with an enable parent too:
        // its own flag counts from then on [documented: DisableAlt, alias SnigOff;
        // EnableAlt, alias SnigOn; `World::keep_own_enable`].
        "disablealt" | "snigoff" | "enablealt" | "snigon" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let on = name == "enablealt" || name == "snigon";
            let fade = num(args, 0, if on { 1.0 } else { 0.0 }) != 0.0;
            let res = g.enable_change(r, |g| {
                g.world.keep_own_enable(r);
                if on {
                    crate::functions::refs::enable_ref(g, r, fade).map(|_| ())
                } else {
                    crate::functions::refs::disable_ref(g, r, fade).map(|_| ())
                }
            });
            if let Err(e) = res {
                return Some(Err(e));
            }
            zero
        }
        // `[newParent]`: the reference's enable parent, none removing it [documented:
        // SetEnableParent, alias SetParentRef; `World::set_enable_parent`].
        "setenableparent" | "setparentref" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let parent = form(args, 0).filter(|&p| p != r);
            if let Err(e) = g.enable_change(r, |g| {
                g.world.set_enable_parent(r, parent);
                Ok(())
            }) {
                return Some(Err(e));
            }
            zero
        }
        // Live actor weapon spread in spread units (`Game::calculated_actor_spread`)
        // [documented: GECK wiki Gun Spread Formula (New Vegas); docs/spec-notes/nvse-stubs-persistent-fall-spread-unique.md].
        "getcalculatedactorspread" => XVal::Num(subject.map_or(0.0, |a| g.calculated_actor_spread(a))),
        // Page marked Incomplete with no summary, arguments or result: 0 [decision: no
        // behaviour to build from; ext-showoff.md §7 asks for the page].
        "isformloading" => zero,
        // 0 for a loaded actor not fading [observed 2026-11-23: Fallout: New Vegas
        // 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, 10 official base masters,
        // `b3_so_01_lvlup_closed_and_status_flags`: `Player.GetActorFadeState` and
        // `rDoc.GetActorFadeState` both returned `0.0` in Doc Mitchell's house; no DLL
        // or source consulted]; -1 for an unloaded actor (or non-actor / missing ref)
        // [observed 2026-11-08: showoff4.fadestate.doc, Doc Mitchell in his unloaded
        // house while the player stood at the airport test ground; hypothesis: unloaded
        // / non-actor is the reason, and other fade states are unknown because
        // NVForever does not fade actors through this state].
        "getactorfadestate" => {
            let loaded_actor = subject.is_some_and(|r| {
                r == FormId::PLAYER_REF
                    || (crate::functions::base_of(g, r)
                        .is_some_and(|b| g.template_source(g.unclone(b), r, |_| false).is_some())
                        && g.present(r)
                        && crate::functions::refs::in_loaded_cell(g, r))
            });
            XVal::Num(if loaded_actor { 0.0 } else { -1.0 })
        }
        _ => return None,
    };
    Some(Ok(v))
}

impl Game<'_> {
    /// The custom hotkey icon path set for `item` by `SetItemHotkeyIconPath`, if any
    /// (empty or whitespace-only paths return `None` so engine consumers fall back to the
    /// item's default hotkey icon) [documented: SetItemHotkeyIconPath; design:
    /// whitespace-only paths ignored by engine consumers].
    pub fn item_hotkey_icon_path(&self, item: FormId) -> Option<&str> {
        self.showoff.hotkey_icons.get(&item).map(String::as_str).filter(|p| !p.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_matrix_examples() {
        let m = |rows: Vec<Vec<f64>>| Matrix { rows, flat: false };
        let a = m(vec![vec![3.0, 4.0], vec![5.0, 6.0]]);
        let b = m(vec![vec![7.0, 8.0], vec![9.0, 10.0]]);
        assert_eq!(matrix_op(&a, "*", &b).unwrap().rows, vec![vec![57.0, 64.0], vec![89.0, 100.0]]);
        assert_eq!(transpose(&a).rows, vec![vec![3.0, 5.0], vec![4.0, 6.0]]);
        assert_eq!(scalar_op(&a, "/", 2.0).unwrap().rows, vec![vec![1.5, 2.0], vec![2.5, 3.0]]);
        assert!(scalar_op(&a, "%", 2.0).is_none());
        let r = Matrix { rows: vec![vec![1.0, 2.0, 3.0]], flat: true };
        assert_eq!(matrix_op(&r, "%", &r).unwrap().rows, vec![vec![1.0, 4.0, 9.0]]);
        assert!(matrix_op(&r, "*", &r).is_none(), "1x3 times 1x3 is not defined");
    }

    #[test]
    fn a_quaternion_and_its_matrix_as_the_game_gives_them() {
        // 90 degrees about Z: the transpose of the right-handed form, in f32.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let r = quat_to_matrix([h, 0.0, 0.0, h]).unwrap();
        assert!((r[0][1] - 1.0).abs() < 1e-6 && (r[1][0] + 1.0).abs() < 1e-6 && (r[2][2] - 1.0).abs() < 1e-6);
        assert_eq!(format!("{:e}", quat_to_matrix([h, 0.0, 0.0, h]).unwrap()[0][0]), "-5.960464477539063e-8");
        // Not normalised.
        let u = quat_to_matrix([2.0, 0.0, 0.0, 0.0]).unwrap();
        assert_eq!([u[0][0], u[1][1], u[2][2]], [7.0, 7.0, 7.0]);
        // Back from the right-handed form.
        let q = matrix_to_quat([[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!((q[0] - h).abs() < 1e-12 && (q[3] - h).abs() < 1e-12);
    }
}
