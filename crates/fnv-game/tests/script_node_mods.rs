//! What mods that move the player's model nodes by script lean on (B42 True Leaning,
//! ISControl): JohnnyGuitar's RenderUpdate handlers read the frame's time, InsertNode's
//! nodes go into models drawn after the call, and GetReticleRange tells nothing under the
//! crosshair apart. Authored scene trees and scripts; no installed data.
use crate::common::model_has_block::*;
use fnv_game::nif_blocks::{ModelView, SceneEdit, SceneNode};
use fnv_script::vm::XVal;
use fnv_sim::FormId;

fn nodes(names: &[&str]) -> Vec<SceneNode> {
    names.iter().enumerate().map(|(i, n)| SceneNode { name: (*n).into(), parent: i.checked_sub(1), translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None }).collect()
}

fn inserts(edits: &[SceneEdit]) -> usize {
    edits.iter().filter(|e| matches!((*e).clone().into_model().1, SceneEdit::Insert { .. })).count()
}

#[test]
fn render_update_handlers_read_the_time_since_the_last_frame() {
    with_game(|g| {
        g.cfg.frame_seconds = 1.0 / 64.0;
        query(g, "SetJohnnyOnRenderUpdateEventHandler 1 (begin function {}\nPlayerREF.AuxVarSetFlt \"*_owned_seconds\" (PlayerREF.AuxVarGetFlt \"*_owned_seconds\" + GetSecondsPassed)\nend) 0");
        for _ in 0..8 {
            g.tick();
        }
        let XVal::Num(seconds) = query(g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*_owned_seconds\")") else { panic!("a number") };
        // The first dispatch has no frame before it.
        assert!((seconds - 7.0 / 64.0).abs() < 1e-9, "{seconds}");
    });
}

#[test]
fn insert_node_goes_into_a_model_drawn_after_the_call_once() {
    with_game(|g| {
        assert_eq!(query(g, "SetFunctionValue (InsertNode OwnedRefA 3 \"OwnedRoot|^Added\")"), XVal::Num(0.0), "no model drawn yet");
        g.scene.set_tree(FIRST, tree("OwnedRoot"));
        g.tick();
        assert_eq!(has(g, "OwnedRefA", "Added"), XVal::Num(1.0));
        assert_eq!(inserts(&g.scene.take_edits()), 1);
        // The engine tells the tree again before it shows the node: no second copy.
        g.scene.set_tree(FIRST, tree("OwnedRoot"));
        g.tick();
        assert_eq!(inserts(&g.scene.take_edits()), 0);
        assert_eq!(has(g, "OwnedRefB", "Added"), XVal::Num(0.0), "another reference keeps its model");
        // The host shows it, then tells a model drawn anew without it (no identity to tell
        // the two apart): it goes in again.
        g.scene.set_tree(FIRST, nodes(&["Added", "OwnedRoot"]));
        g.tick();
        assert_eq!(inserts(&g.scene.take_edits()), 0);
        g.scene.set_tree(FIRST, tree("OwnedRoot"));
        g.tick();
        assert_eq!(inserts(&g.scene.take_edits()), 1);
        assert_eq!(has(g, "OwnedRefA", "Added"), XVal::Num(1.0));
    });
}

#[test]
fn insert_node_on_the_player_goes_into_each_model_with_the_target() {
    with_game(|g| {
        g.scene.use_separate_player_models();
        g.scene.set_tree_for(FormId::PLAYER_REF, ModelView::FirstPerson, nodes(&["Scene Root", "Bip01 Looking", "Camera1st"]));
        g.scene.set_tree_for(FormId::PLAYER_REF, ModelView::ThirdPerson, nodes(&["Scene Root", "Bip01 Head"]));
        assert_eq!(query(g, "SetFunctionValue (InsertNode PlayerREF 3 \"Camera1st|^OwnedCam\")"), XVal::Num(1.0));
        assert_eq!(query(g, "SetFunctionValue (InsertNode PlayerREF 3 \"Bip01 Head|^OwnedHead\")"), XVal::Num(1.0));
        let first = g.scene.node_for_model(FormId::PLAYER_REF, ModelView::FirstPerson, "OwnedCam");
        assert!(first.is_some_and(|n| n.parent == Some(1)), "between Camera1st and its parent");
        assert!(g.scene.node_for_model(FormId::PLAYER_REF, ModelView::ThirdPerson, "OwnedCam").is_none(), "no Camera1st there");
        assert!(g.scene.node_for_model(FormId::PLAYER_REF, ModelView::ThirdPerson, "OwnedHead").is_some());
        assert_eq!(inserts(&g.scene.take_edits()), 2);
    });
}

#[test]
fn reticle_range_is_minus_one_with_nothing_under_the_crosshair() {
    with_game(|g| {
        assert_eq!(query(g, "SetFunctionValue (GetReticleRange)"), XVal::Num(-1.0));
    });
}

/// `OwnedRefA` drawn as `model` (an identity, or none) with InsertNode's `Added` shown.
fn with_shown_node(g: &mut fnv_game::game::Game<'_>, model: Option<u64>) {
    tell(g, model, &["OwnedRoot"]);
    query(g, "InsertNode OwnedRefA 3 \"OwnedRoot|Added\"");
    tell(g, model, &["OwnedRoot", "Added"]);
    g.tick();
    g.scene.take_edits();
}

fn tell(g: &mut fnv_game::game::Game<'_>, model: Option<u64>, names: &[&str]) {
    match model {
        Some(model) => g.scene.set_identified_model_for(FIRST, ModelView::Reference, model, nodes(names)),
        None => g.scene.set_tree(FIRST, nodes(names)),
    }
}

/// A frame passes: how many nodes it inserted.
fn frame(g: &mut fnv_game::game::Game<'_>) -> usize {
    g.tick();
    inserts(&g.scene.take_edits())
}

#[test]
fn a_removed_node_stays_removed_in_that_model() {
    for model in [None, Some(7)] {
        with_game(|g| {
            with_shown_node(g, model);
            query(g, "OwnedRefA.RemoveNifBlock \"added\"");
            assert_eq!(frame(g), 0, "{model:?}: the next frame");
            // The host applies the removal late: it tells the node once more, then without it.
            tell(g, model, &["OwnedRoot", "Added"]);
            assert_eq!(frame(g), 0, "{model:?}: still shown");
            tell(g, model, &["OwnedRoot"]);
            assert_eq!(frame(g), 0, "{model:?}: shown no more");
            assert_eq!(has(g, "OwnedRefA", "Added"), XVal::Num(0.0));
        });
    }
}

#[test]
fn a_node_removed_with_the_one_above_it_stays_removed() {
    with_game(|g| {
        tell(g, Some(7), &["OwnedRoot"]);
        query(g, "InsertNode OwnedRefA 3 \"OwnedRoot|^Above\"");
        tell(g, Some(7), &["Scene Root", "Holder", "Above"]);
        g.tick();
        g.scene.take_edits();
        query(g, "OwnedRefA.RemoveNifBlock \"Holder\"");
        // The target stands elsewhere in the model, so the entry could go in again.
        tell(g, Some(7), &["Scene Root", "OwnedRoot"]);
        assert_eq!(frame(g), 0);
    });
}

#[test]
fn a_renamed_node_is_not_inserted_again_under_its_old_name() {
    for model in [None, Some(7)] {
        with_game(|g| {
            with_shown_node(g, model);
            query(g, "OwnedRefA.RenameNifBlock \"Added\" \"Renamed\"");
            assert_eq!(frame(g), 0, "{model:?}: the next frame");
            tell(g, model, &["OwnedRoot", "Renamed"]);
            assert_eq!(frame(g), 0, "{model:?}: told renamed");
        });
    }
}

#[test]
fn a_removed_node_comes_back_with_a_new_model_or_a_new_insert_node() {
    with_game(|g| {
        with_shown_node(g, Some(7));
        query(g, "OwnedRefA.RemoveNifBlock \"Added\"");
        tell(g, Some(8), &["OwnedRoot"]);
        assert_eq!(frame(g), 1, "another model identity");
    });
    with_game(|g| {
        with_shown_node(g, None);
        query(g, "OwnedRefA.RemoveNifBlock \"Added\"");
        g.scene.forget(FIRST);
        tell(g, None, &["OwnedRoot"]);
        assert_eq!(frame(g), 1, "the 3D was gone");
    });
    with_game(|g| {
        with_shown_node(g, None);
        query(g, "OwnedRefA.RemoveNifBlock \"Added\"");
        g.scene.forget_model(FIRST, ModelView::Reference);
        tell(g, None, &["OwnedRoot"]);
        assert_eq!(frame(g), 1, "the model was gone");
    });
    for model in [None, Some(7)] {
        with_game(|g| {
            // Removed before the host ever showed it, and asked for again.
            tell(g, model, &["OwnedRoot"]);
            query(g, "InsertNode OwnedRefA 3 \"OwnedRoot|Added\"\nOwnedRefA.RemoveNifBlock \"Added\"");
            g.scene.take_edits();
            assert_eq!(frame(g), 0, "{model:?}: removed before it was shown");
            query(g, "InsertNode OwnedRefA 3 \"OwnedRoot|Added\"");
            assert_eq!(inserts(&g.scene.take_edits()), 1, "{model:?}: asked for again");
        });
    }
}

#[test]
fn a_removal_holds_in_the_player_model_it_was_made_in() {
    with_game(|g| {
        let player = FormId::PLAYER_REF;
        g.scene.use_separate_player_models();
        for (view, id) in [(ModelView::ThirdPerson, 3), (ModelView::FirstPerson, 1)] {
            g.scene.set_identified_model_for(player, view, id, nodes(&["OwnedRoot"]));
        }
        query(g, "InsertNode PlayerREF 3 \"OwnedRoot|Added\"");
        for (view, id) in [(ModelView::ThirdPerson, 3), (ModelView::FirstPerson, 1)] {
            g.scene.set_identified_model_for(player, view, id, nodes(&["OwnedRoot", "Added"]));
        }
        g.tick();
        g.scene.take_edits();
        query(g, "PlayerREF.RemoveNifBlock \"Added\" 2");
        assert_eq!(frame(g), 0);
        assert!(g.scene.node_for_model(player, ModelView::FirstPerson, "Added").is_none());
        assert!(g.scene.node_for_model(player, ModelView::ThirdPerson, "Added").is_some());
    });
}

#[test]
fn get_point_ray_cast_pos_uses_right_handed_angles_and_writes_endpoint_on_miss() {
    use fnv_game::ray::{with_caster, CastKind, Hit};
    with_game(|g| {
        type CastLog = Vec<([f32; 3], [f32; 3], CastKind)>;
        let captured: std::cell::RefCell<CastLog> = Default::default();
        let miss_caster = |from: [f32; 3], to: [f32; 3], kind: CastKind| -> Option<Hit> {
            captured.borrow_mut().push((from, to, kind));
            None
        };
        with_caster(&miss_caster, || {
            // Near world origin (0, 0, 100): an open-sky miss returns 1 and overwrites
            // pre-seeded output variables with `origin + dir * maxRange` [observed: w6_01, w6_02].
            // Cardinal directions: 0 = +Y (North), -90 = +X (East), +90 = -X (West),
            // 180 = -Y (South); pitch -90 = -Z (Down), +90 = +Z (Up); diagonal (30, -90)
            // confirms Rz(rotZ) * Rx(rotX) * +Y.
            let cases: &[(f64, f64, [f64; 3])] = &[
                (0.0, 0.0, [0.0, 300.0, 100.0]),
                (0.0, -90.0, [300.0, 0.0, 100.0]),
                (0.0, 90.0, [-300.0, 0.0, 100.0]),
                (0.0, 180.0, [0.0, -300.0, 100.0]),
                (-90.0, 0.0, [0.0, 0.0, -200.0]),
                (90.0, 0.0, [0.0, 0.0, 400.0]),
                (45.0, 0.0, [0.0, 300.0 * 45.0_f64.to_radians().cos(), 100.0 + 300.0 * 45.0_f64.to_radians().sin()]),
                (-45.0, 0.0, [0.0, 300.0 * 45.0_f64.to_radians().cos(), 100.0 - 300.0 * 45.0_f64.to_radians().sin()]),
                (30.0, -90.0, [300.0 * 30.0_f64.to_radians().cos(), 0.0, 100.0 + 150.0]),
            ];
            for &(rx, rz, exp) in cases {
                let res = query(
                    g,
                    &format!(
                        "float fX = 111\nfloat fY = 222\nfloat fZ = 333\nint ok = GetPointRayCastPos 0 0 100 ({rx}) ({rz}) fX fY fZ 19 300\n\
                         OwnedRefA.AuxVarSetFlt \"*rx\" fX\nOwnedRefA.AuxVarSetFlt \"*ry\" fY\nOwnedRefA.AuxVarSetFlt \"*rz\" fZ\n\
                         OwnedRefA.AuxVarSetFlt \"*rd\" (GetVector3DDistance fX fY fZ 0 0 100)\nSetFunctionValue ok"
                    ),
                );
                assert_eq!(res, XVal::Num(1.0), "open-sky miss at ({rx}, {rz}) returns 1 (observed w6_01/w6_02)");
                let got = ["*rx", "*ry", "*rz"].map(|k| match query(g, &format!("SetFunctionValue (OwnedRefA.AuxVarGetFlt \"{k}\")")) {
                    XVal::Num(n) => n,
                    other => panic!("{k}: {other:?}"),
                });
                for axis in 0..3 {
                    assert!(
                        (got[axis] - exp[axis]).abs() < 1e-2,
                        "angles ({rx}, {rz}) axis {axis}: got {:.3}, expected {:.3}",
                        got[axis],
                        exp[axis]
                    );
                }
                let XVal::Num(dist) = query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*rd\")") else { panic!("distance") };
                assert!((dist - 300.0).abs() < 1e-2, "angles ({rx}, {rz}) miss distance: {dist}");
            }
            assert!(captured.borrow().iter().all(|(_, _, k)| *k == CastKind::CameraClearance), "flag 19 maps to CameraClearance");

            // Explicit maxRange = 0 yields origin (reach = 0, no physics cast); explicit -1 casts
            // backward 1 unit; omitted maxRange (8-arg and 9-arg) defaults to 50_000 [observed: w6_02].
            captured.borrow_mut().clear();
            let r0 = query(
                g,
                "float fX = 111\nfloat fY = 222\nfloat fZ = 333\nint ok = GetPointRayCastPos 10 20 100 90 0 fX fY fZ 19 0\n\
                 OwnedRefA.AuxVarSetFlt \"*rx\" fX\nOwnedRefA.AuxVarSetFlt \"*ry\" fY\nOwnedRefA.AuxVarSetFlt \"*rz\" fZ\nSetFunctionValue ok",
            );
            assert_eq!(r0, XVal::Num(1.0));
            assert!(captured.borrow().is_empty(), "maxRange=0 must not invoke zero-length cast");
            assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*rx\")"), XVal::Num(10.0));
            assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*ry\")"), XVal::Num(20.0));
            assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*rz\")"), XVal::Num(100.0));

            let rn1 = query(
                g,
                "float fX = 111\nfloat fY = 222\nfloat fZ = 333\nint ok = GetPointRayCastPos 10 20 100 90 0 fX fY fZ 19 (-1)\n\
                 OwnedRefA.AuxVarSetFlt \"*rz\" fZ\nSetFunctionValue ok",
            );
            assert_eq!(rn1, XVal::Num(1.0));
            assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*rz\")"), XVal::Num(99.0));

            let rom9 = query(
                g,
                "float fX = 111\nfloat fY = 222\nfloat fZ = 333\nint ok = GetPointRayCastPos 10 20 100 90 0 fX fY fZ 19\n\
                 OwnedRefA.AuxVarSetFlt \"*rz\" fZ\nSetFunctionValue ok",
            );
            assert_eq!(rom9, XVal::Num(1.0));
            assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*rz\")"), XVal::Num(50_100.0));

            let rom8 = query(
                g,
                "float fX = 111\nfloat fY = 222\nfloat fZ = 333\nint ok = GetPointRayCastPos 10 20 100 90 0 fX fY fZ\n\
                 OwnedRefA.AuxVarSetFlt \"*rz\" fZ\nSetFunctionValue ok",
            );
            assert_eq!(rom8, XVal::Num(1.0));
            assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*rz\")"), XVal::Num(50_100.0));

            // collisionFlag routing [observed: w6_03]: 0/1/6/-1/omitted -> World; 8/19 -> CameraClearance.
            captured.borrow_mut().clear();
            for (flag, expected_kind) in [
                (0, CastKind::World),
                (1, CastKind::World),
                (6, CastKind::World),
                (-1, CastKind::World),
                (8, CastKind::CameraClearance),
                (19, CastKind::CameraClearance),
            ] {
                query(
                    g,
                    &format!("float fX\nfloat fY\nfloat fZ\nGetPointRayCastPos 0 0 100 (-90) 0 fX fY fZ ({flag}) 300"),
                );
                let got_kind = captured.borrow().last().unwrap().2;
                assert_eq!(got_kind, expected_kind, "collisionFlag {flag}");
            }
        });

        // A hit returns 1 and writes the hit coordinates.
        let hit_caster = |from: [f32; 3], to: [f32; 3], _kind: CastKind| -> Option<Hit> {
            let mid = [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5, (from[2] + to[2]) * 0.5];
            Some(Hit { distance: 150.0, point: mid, form: None, material: None })
        };
        with_caster(&hit_caster, || {
            let res = query(
                g,
                "float fX\nfloat fY\nfloat fZ\nint ok = GetPointRayCastPos 0 0 100 0 -90 fX fY fZ 19 300\n\
                 OwnedRefA.AuxVarSetFlt \"*hx\" fX\nSetFunctionValue ok",
            );
            assert_eq!(res, XVal::Num(1.0));
            let XVal::Num(hx) = query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*hx\")") else { panic!("hx") };
            assert!((hx - 150.0).abs() < 1e-2, "hit X east: {hx}");
        });
    });
}

#[test]
fn b42_leaning_manual_and_contextual_cover_lean_end_to_end() {
    use fnv_game::functions::senses::Reticle;
    use fnv_game::ray::{with_caster, CastKind, Hit};
    with_game(|g| {
        let player = FormId::PLAYER_REF;
        g.cfg.frame_seconds = 1.0 / 60.0;
        g.senses.first_person = true;
        g.scene.use_separate_player_models();
        let first_nodes = vec![
            SceneNode { name: "Scene Root".into(), parent: None, translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01".into(), parent: Some(0), translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01 NonAccum".into(), parent: Some(1), translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01 Looking".into(), parent: Some(2), translation: [0.0, 0.0, 118.0], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01 Translate".into(), parent: Some(3), translation: [0.0, 0.0, -118.0], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Camera1st".into(), parent: Some(3), translation: [0.0, -3.741, 0.0], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
        ];
        g.scene.set_identified_model_for(player, ModelView::FirstPerson, 100, first_nodes);
        g.scene.set_identified_model_for(player, ModelView::ThirdPerson, 200, nodes(&["Scene Root", "Bip01 Head"]));

        // Authored B42-True-Leaning-style pipeline: ModKey=257 (RMB), KeyLeft=16 (Q),
        // KeyRight=18 (E), Contextual=1, Angle=18, CamLeanAmount=0.33, Sidestep=22.
        query(
            g,
            r#"
OwnedRefA.AuxVarSetFlt "*L_Ang" 18
OwnedRefA.AuxVarSetFlt "*L_Cam" 0.33
OwnedRefA.AuxVarSetFlt "*L_Step" 22
OwnedRefA.AuxVarSetFlt "*L_Ctx" 1
OwnedRefA.AuxVarSetFlt "*L_Left" 16
OwnedRefA.AuxVarSetFlt "*L_Right" 18
OwnedRefA.AuxVarSetFlt "*L_Mod" 257
InsertNode PlayerREF 3 "Camera1st|^LeanCam"
InsertNode PlayerREF 3 "Camera1st|LeanCamB"
InsertNode PlayerREF 3 "Bip01 Translate|^LeanTrn"
InsertNode PlayerREF 3 "Bip01 Looking|^LeanLkn"

SetOnKeyDownEventHandler (begin function {int k}
    if eval menumode < 1 && OwnedRefA.AuxVarGetFlt "*L_Mod"
        OwnedRefA.AuxVarErase "*L_Side"
        OwnedRefA.AuxVarSetFlt "*L_ModHeld" 1
    endif
end) 1 (OwnedRefA.AuxVarGetFlt "*L_Mod")

SetOnKeyUpEventHandler (begin function {int k}
    OwnedRefA.AuxVarErase "*L_ModHeld"
    OwnedRefA.AuxVarErase "*L_Side"
end) 1 (OwnedRefA.AuxVarGetFlt "*L_Mod")

SetOnKeyDownEventHandler (begin function {int k}
    if eval menumode < 1 && (!OwnedRefA.AuxVarGetFlt "*L_Mod" || OwnedRefA.AuxVarGetFlt "*L_ModHeld")
        OwnedRefA.AuxVarSetFlt "*L_Side" 1
    endif
end) 1 (OwnedRefA.AuxVarGetFlt "*L_Left")

SetOnKeyUpEventHandler (begin function {int k}
    OwnedRefA.AuxVarErase "*L_Side"
end) 1 (OwnedRefA.AuxVarGetFlt "*L_Left")

SetOnKeyDownEventHandler (begin function {int k}
    if eval menumode < 1 && (!OwnedRefA.AuxVarGetFlt "*L_Mod" || OwnedRefA.AuxVarGetFlt "*L_ModHeld")
        OwnedRefA.AuxVarSetFlt "*L_Side" -1
    endif
end) 1 (OwnedRefA.AuxVarGetFlt "*L_Right")

SetOnKeyUpEventHandler (begin function {int k}
    OwnedRefA.AuxVarErase "*L_Side"
end) 1 (OwnedRefA.AuxVarGetFlt "*L_Right")

SetOnControlDownEventHandler (begin function {int c}
    if eval menumode < 1 && OwnedRefA.AuxVarGetFlt "*L_Ctx" && !OwnedRefA.AuxVarGetFlt "*L_Side" && (GetReticleRange 19 300) < 130
        c = 0
        OwnedRefA.AuxVarErase "*L_CtxSide"
        while (c += 1) < 3
            DispatchEventAlt "LeanProbe" c
            int side = OwnedRefA.AuxVarGetFlt "*L_CtxSide"
            if side != 0
                OwnedRefA.AuxVarSetFlt "*L_Side" side
                OwnedRefA.AuxVarSetFlt "*L_CtxSide" 1
                break
            endif
        loop
    endif
end) 1 6

SetOnControlUpEventHandler (begin function {int c}
    if eval menumode < 1 && OwnedRefA.AuxVarGetFlt "*L_CtxSide"
        OwnedRefA.AuxVarErase "*L_Side"
        OwnedRefA.AuxVarErase "*L_CtxSide"
    endif
end) 1 6

SetEventHandlerAlt "LeanProbe" (begin function {int b}
    if b != 0
        eval b == 2 && (b = -1)
        PlayerREF.SetNifTrans "LeanCamB" (18 * b) 0 0 2
        DispatchEventAlt "LeanCast" b 0 (-PlayerREF.GetAngle X)
    endif
end)

SetEventHandlerAlt "LeanCast" (begin function {int gSide int hSide float pitch}
    PlayerREF.GetNifTransAlt "LeanCamB" (float x0) (float y0) (float z0) 1 2
    if gSide == 0
        PlayerREF.GetPosEx (x0) (y0) (float dist)
    endif
    GetPointRayCastPos x0 y0 z0 pitch (-PlayerREF.GetAngle Z + (80 * hSide)) (float x1) (float y1) (float z1) 19 300
    dist = GetVector3DDistance x1 y1 z1 x0 y0 z0
    if hSide != 0
        OwnedRefA.AuxVarSetFlt "*L_Clear" dist
    else
        OwnedRefA.AuxVarSetFlt "*L_CtxSide" ((dist > 150) * -gSide)
    endif
end)

SetJohnnyOnRenderUpdateEventHandler 1 (begin function {}
    if menumode < 1
        float timer = OwnedRefA.AuxVarGetFlt "*L_Timer"
        if OwnedRefA.AuxVarGetFlt "*L_Side" != 0
            timer = 0.7
        endif
        if timer > 0
            float dt = GetSecondsPassed
            timer -= dt
            int side = OwnedRefA.AuxVarGetFlt "*L_Side"
            DispatchEventAlt "LeanCast" 0 side 0
            float clr = clamp (((OwnedRefA.AuxVarGetFlt "*L_Clear") - 20) / 20.0) 0 1
            float l1 = Lerp (OwnedRefA.AuxVarGetFlt "*L_Lerp1") clr (dt * 10)
            OwnedRefA.AuxVarSetFlt "*L_Lerp1" l1
            float l2 = Lerp (OwnedRefA.AuxVarGetFlt "*L_Lerp2") side (dt * 10)
            OwnedRefA.AuxVarSetFlt "*L_Lerp2" l2
            float u = l2 * l1
            float ang = OwnedRefA.AuxVarGetFlt "*L_Ang" * u
            PlayerREF.SetNifRot "LeanTrn" 0 ang 0 0 2
            PlayerREF.SetNifRot "LeanCam" 0 (ang * OwnedRefA.AuxVarGetFlt "*L_Cam") 0 0 2
            PlayerREF.SetNifTrans "LeanLkn" (OwnedRefA.AuxVarGetFlt "*L_Step" * -u) 0 (-10 * abs u) 2
            OwnedRefA.AuxVarSetFlt "*L_Timer" timer
        endif
    endif
end) 0 4
"#,
        );

        // Part 1 — Manual lean around cover near world origin (0, 0, 0) facing North (0 deg):
        // A solid wall stands immediately to the player's RIGHT at x = +12 (open air to the LEFT).
        let right_wall = |from: [f32; 3], to: [f32; 3], _kind: CastKind| -> Option<Hit> {
            let dx = to[0] - from[0];
            if dx > 1e-4 && from[0] < 12.0 && to[0] >= 12.0 {
                let t = (12.0 - from[0]) / dx;
                let p = [12.0, from[1] + (to[1] - from[1]) * t, from[2] + (to[2] - from[2]) * t];
                let d = ((p[0] - from[0]).powi(2) + (p[1] - from[1]).powi(2) + (p[2] - from[2]).powi(2)).sqrt();
                return Some(Hit { distance: d, point: p, form: None, material: None });
            }
            None
        };
        g.senses.reticle = Some(Reticle { start: [0.0, 0.0, 118.0], dir: [0.0, 1.0, 0.0], hit: Some(500.0), limb: None });
        with_caster(&right_wall, || {
            g.tick();
            // Hold ModKey (257) + KeyLeft (16): clearance ray goes LEFT (-X, open air -> 300),
            // so left lean succeeds without hitting the wall on the right.
            g.input.held.insert(257);
            g.tick();
            g.input.held.insert(16);
            for _ in 0..8 {
                g.tick();
            }
        });
        let cam_left = g.scene.node_for_model(player, ModelView::FirstPerson, "LeanCam").unwrap();
        let lkn_left = g.scene.node_for_model(player, ModelView::FirstPerson, "LeanLkn").unwrap();
        assert!(cam_left.rotation[1] > 1.0, "left lean must roll camera positively: {:?}", cam_left.rotation);
        assert!(lkn_left.translation[0] < -5.0, "left lean must sidestep left (-X): {:?}", lkn_left.translation);

        // Release Q and press E (lean RIGHT into the wall at x = +12): clearance ray hits wall
        // at ~12.2 units (< 20), driving clearance factor to 0 and preventing leaning into cover.
        with_caster(&right_wall, || {
            g.input.held.remove(&16);
            g.tick();
            g.input.held.insert(18);
            for _ in 0..40 {
                g.tick();
            }
        });
        let XVal::Num(right_clear) = query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*L_Clear\")") else { panic!("clear") };
        assert!(right_clear < 20.0, "right lean clearance ray must hit right wall (< 20): {right_clear}");
        let lkn_blocked = g.scene.node_for_model(player, ModelView::FirstPerson, "LeanLkn").unwrap();
        assert!(lkn_blocked.translation[0].abs() < 0.25, "wall on right must block leaning right: {:?}", lkn_blocked.translation);

        // Part 2 — Contextual aim lean while facing EAST (GetAngle Z = 90 deg) behind a wall
        // at x = +50 that covers y >= 0 (ahead & left/North) and ends for y < 0 (open on right/South).
        with_caster(&right_wall, || {
            g.input.held.remove(&18);
            g.input.held.remove(&257);
            for _ in 0..40 {
                g.tick();
            }
        });
        query(g, "PlayerREF.SetAngle Z 90");
        // Mirror root rotation for East-facing player (`posed_root` = Rz(-90 deg), matching
        // `nif_mirror` `root_told` = Rz(-90) * Rz(-90) = Rz(-180) stored as transpose +180),
        // published under a new model identity (101) so `insert_runtime_nodes` also exercises
        // re-inserting the lean nodes after a player-body rebuild.
        let east_nodes = vec![
            SceneNode { name: "Scene Root".into(), parent: None, translation: [0.0; 3], rotation: [0.0, 0.0, 180.0], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01".into(), parent: Some(0), translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01 NonAccum".into(), parent: Some(1), translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01 Looking".into(), parent: Some(2), translation: [0.0, 0.0, 118.0], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Bip01 Translate".into(), parent: Some(3), translation: [0.0, 0.0, -118.0], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
            SceneNode { name: "Camera1st".into(), parent: Some(3), translation: [0.0, -3.741, 0.0], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None },
        ];
        g.scene.set_identified_model_for(player, ModelView::FirstPerson, 101, east_nodes);
        g.tick();
        g.senses.reticle = Some(Reticle { start: [0.0, 0.0, 118.0], dir: [1.0, 0.0, 0.0], hit: Some(50.0), limb: None });
        let east_corner = |from: [f32; 3], to: [f32; 3], _kind: CastKind| -> Option<Hit> {
            let dx = to[0] - from[0];
            if dx > 1e-4 && from[0] < 50.0 && to[0] >= 50.0 {
                let t = (50.0 - from[0]) / dx;
                let y = from[1] + (to[1] - from[1]) * t;
                if y >= 0.0 {
                    let p = [50.0, y, from[2] + (to[2] - from[2]) * t];
                    let d = ((p[0] - from[0]).powi(2) + (p[1] - from[1]).powi(2) + (p[2] - from[2]).powi(2)).sqrt();
                    return Some(Hit { distance: d, point: p, form: None, material: None });
                }
            }
            None
        };
        with_caster(&east_corner, || {
            // Aim (Control 6 via mouse 257): B42LB checks +18 (right/South, y = -18 -> East ray
            // misses wall -> dist = 300 > 150) and triggers contextual right lean (*L_Side = -1).
            g.input.held.insert(257);
            for _ in 0..10 {
                g.tick();
            }
        });
        assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*L_CtxSide\")"), XVal::Num(1.0), "contextual lean must engage on open East corner");
        assert_eq!(query(g, "SetFunctionValue (OwnedRefA.AuxVarGetFlt \"*L_Side\")"), XVal::Num(-1.0), "contextual lean must choose open right side (-1)");
        let lkn_ctx = g.scene.node_for_model(player, ModelView::FirstPerson, "LeanLkn").unwrap();
        assert!(lkn_ctx.translation[0] > 5.0, "contextual right lean must sidestep right (+X): {:?}", lkn_ctx.translation);
    });
}
