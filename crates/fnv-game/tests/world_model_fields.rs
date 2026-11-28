//! Independently authored record data for the GetModelPath world-object boundary.
use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    GameData,
    game::{Game, GameConfig, RunCtx},
};
use fnv_script::{FunctionTable, vm::XVal};
use fnv_sim::{FormStore, RefTable, SimConfig, World};

const MODEL: &str = "Owned\\Original Model.NIF";
const TYPES: [([u8; 4], &str); 6] = [
    (*b"STAT", "OwnStaticModel"),
    (*b"MSTT", "OwnMovableModel"),
    (*b"FURN", "OwnFurnitureModel"),
    (*b"DOOR", "OwnDoorModel"),
    (*b"TACT", "OwnTalkingModel"),
    (*b"ACTI", "OwnActivatorModel"),
];

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let mut bytes = tes4();
    bytes.extend(top(
        b"NPC_",
        &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]),
    ));
    for (index, (sig, name)) in TYPES.iter().enumerate() {
        bytes.extend(top(
            sig,
            &record(
                sig,
                0,
                0xD00 + index as u32,
                &[sub(b"EDID", z(name)), sub(b"MODL", z(MODEL))],
            ),
        ));
    }
    bytes.extend(top(
        b"STAT",
        &record(b"STAT", 0, 0xD10, &[sub(b"EDID", z("OwnNoModel"))]),
    ));
    bytes.extend(top(
        b"MISC",
        &record(
            b"MISC",
            0,
            0xD11,
            &[sub(b"EDID", z("OwnMiscModel")), sub(b"MODL", z(MODEL))],
        ),
    ));
    let plugins = vec![Plugin::parse(&bytes, "OwnedModels.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let functions = root.join("script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(
        &lo,
        &schema,
        FunctionTable::load(&functions).unwrap(),
        &json,
        &refs,
    );
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    Game::new(
        &f.data,
        World::new(&f.forms, &f.refs, SimConfig::default()),
        GameConfig {
            strict: true,
            ..Default::default()
        },
    )
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(
        &format!("Begin Function {{}}\n{body}\nEnd"),
        RunCtx::default(),
    )
    .unwrap()
}

#[test]
fn authored_world_object_models_are_readable_without_path_rewriting() {
    let f = fixture();
    let mut g = game(&f);
    for (_, name) in TYPES {
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {name})")),
            XVal::Str(MODEL.into()),
            "authored MODL for {name}"
        );
    }
    assert_eq!(
        q(&mut g, "SetFunctionValue (GetModelPath OwnNoModel)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        q(&mut g, "SetFunctionValue (GetModelPath 0)"),
        XVal::Str(String::new())
    );
}

#[test]
fn clone_model_edits_leave_the_original_and_other_games_independent() {
    let f = fixture();
    let mut g = game(&f);
    let XVal::Ref(clone) = q(
        &mut g,
        "ref ownClone\nlet ownClone := TempCloneForm OwnStaticModel 1\nSetFunctionValue ownClone",
    ) else {
        panic!("a clone reference is required")
    };
    assert_eq!(
        q(
            &mut g,
            &format!("SetFunctionValue (GetModelPath {clone:#010X})")
        ),
        XVal::Str(MODEL.into())
    );
    q(
        &mut g,
        &format!(r#"SetModelPath "owned\clone.nif" {clone:#010X}"#),
    );
    assert_eq!(
        q(
            &mut g,
            &format!("SetFunctionValue (GetModelPath {clone:#010X})")
        ),
        XVal::Str("owned\\clone.nif".into())
    );
    assert_eq!(
        q(&mut g, "SetFunctionValue (GetModelPath OwnStaticModel)"),
        XVal::Str(MODEL.into())
    );
    let mut next = game(&f);
    assert_eq!(
        q(&mut next, "SetFunctionValue (GetModelPath OwnStaticModel)"),
        XVal::Str(MODEL.into())
    );
}

#[test]
fn a_placed_reference_reads_its_authored_base_model() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(
        q(
            &mut g,
            "ref ownPlaced\nlet ownPlaced := player.PlaceAtMe OwnStaticModel\nSetFunctionValue (ownPlaced.GetModelPath)"
        ),
        XVal::Str(MODEL.into())
    );
}

#[test]
fn tempclone_snapshots_model_path_at_creation_for_stat_and_misc() {
    let f = fixture();
    for (edid, pre, post, cl_edit) in [
        ("OwnStaticModel", "w4g_pre_stat.nif", "w4g_post_stat.nif", "w4g_cl_stat.nif"),
        ("OwnMiscModel", "w4g_pre_misc.nif", "w4g_post_misc.nif", "w4g_cl_misc.nif"),
    ] {
        let mut g = game(&f);
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {edid})")),
            XVal::Str(MODEL.into())
        );

        // Pre-clone edit on the source is inherited by the clone at creation time.
        q(&mut g, &format!(r#"SetModelPath "{pre}" {edid}"#));
        let XVal::Ref(cl1) = q(
            &mut g,
            &format!("ref c\nlet c := TempCloneForm {edid} 1\nSetFunctionValue c"),
        ) else {
            panic!("expected clone for {edid}")
        };
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {cl1:#010X})")),
            XVal::Str(pre.into())
        );

        // Post-clone edit on the source does NOT bleed into the existing clone.
        q(&mut g, &format!(r#"SetModelPath "{post}" {edid}"#));
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {edid})")),
            XVal::Str(post.into())
        );
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {cl1:#010X})")),
            XVal::Str(pre.into()),
            "post-clone source edit bled into {edid} clone"
        );

        // Clone-of-clone inherits parent clone's snapshot and stays isolated from later parent edits.
        let XVal::Ref(cl2) = q(
            &mut g,
            &format!(
                "ref c1\nref c2\nlet c1 := StringToRef \"{cl1:08X}\"\nlet c2 := TempCloneForm c1 1\nSetFunctionValue c2"
            ),
        ) else {
            panic!("expected clone-of-clone for {edid}")
        };
        assert_ne!(cl2, 0, "clone-of-clone must allocate a non-null FormId for {edid}");
        assert_ne!(cl2, cl1);
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {cl2:#010X})")),
            XVal::Str(pre.into())
        );
        q(&mut g, &format!(r#"SetModelPath "{cl_edit}" {cl1:#010X}"#));
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {cl1:#010X})")),
            XVal::Str(cl_edit.into())
        );
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {cl2:#010X})")),
            XVal::Str(pre.into()),
            "post-clone parent-clone edit bled into {edid} child clone"
        );
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (GetModelPath {edid})")),
            XVal::Str(post.into())
        );
    }

    // Cloning an unedited source or model-less source stays isolated when the source is edited later.
    let mut g = game(&f);
    let XVal::Ref(unedited_cl) = q(
        &mut g,
        "ref c\nlet c := TempCloneForm OwnDoorModel 1\nSetFunctionValue c",
    ) else {
        panic!("expected door clone")
    };
    let XVal::Ref(empty_cl) = q(
        &mut g,
        "ref c\nlet c := TempCloneForm OwnNoModel 1\nSetFunctionValue c",
    ) else {
        panic!("expected model-less clone")
    };
    q(&mut g, r#"SetModelPath "late_door.nif" OwnDoorModel"#);
    q(&mut g, r#"SetModelPath "late_empty.nif" OwnNoModel"#);
    assert_eq!(
        q(&mut g, &format!("SetFunctionValue (GetModelPath {unedited_cl:#010X})")),
        XVal::Str(MODEL.into())
    );
    assert_eq!(
        q(&mut g, &format!("SetFunctionValue (GetModelPath {empty_cl:#010X})")),
        XVal::Str(String::new())
    );
}
