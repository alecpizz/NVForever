//! Source-name actor queries and merchant clearing (actor-query-context.md).
use crate::game::Game;
use fnv_script::vm::XVal;
use fnv_sim::FormId;

pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    if name == "getplayergrabbedref" {
        return Some(Ok(XVal::Ref(g.senses.grabbed.map_or(0, |r| r.0))));
    }
    if !matches!(name, "getpackagetarget" | "removemerchantcontainer") {
        return None;
    }
    let requested = if name == "removemerchantcontainer" {
        match args.first() { Some(XVal::Ref(id)) => Some(FormId(*id)), _ => None }
    } else { subject };
    let actor = requested.filter(|&r| r.0 != 0 && g.def_following(r, |_| false).is_some());
    let Some(actor) = actor else {
        return Some(Ok(if name == "getpackagetarget" { XVal::Ref(0) } else { XVal::Num(0.0) }));
    };
    if name == "removemerchantcontainer" {
        let extra = g.extra.entry(actor).or_default();
        extra.merchant_container = None;
        extra.merchant_container_cleared = true;
        return Some(Ok(XVal::Num(0.0)));
    }
    let conversation = g.dialogue.as_ref().and_then(|d| {
        if actor == FormId::PLAYER_REF { Some(d.actor) }
        else if actor == d.actor { Some(FormId::PLAYER_REF) }
        else { None }
    }).or_else(|| g.speech.iter().filter(|s| s.conversation.is_some()).find_map(|s| {
        if s.speaker == actor { s.target }
        else if s.target == Some(actor) { Some(s.speaker) }
        else { None }
    }));
    let target = conversation.or_else(|| {
        let package = g.current_package(actor)?;
        g.package_def(&package).and_then(|def| g.package_target(actor, def))
    });
    Some(Ok(XVal::Ref(target.map_or(0, |r| r.0))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::*;
    use crate::{game::{GameConfig, RunCtx}, package::{PackageDef, Target, TargetKind}, GameData};
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_script::FunctionTable;
    use fnv_sim::{FormStore, RefTable, SimConfig, World};

    struct Fixture { data: GameData, forms: FormStore, refs: RefTable }

    fn fixture() -> Fixture {
        let actor = |sig: &[u8; 4], id: u32| record(sig, 0, id, &[sub(b"EDID", z(&format!("Actor{id:X}")))]);
        let list = |sig: &[u8; 4], id: u32, item: u32| record(sig, 0, id, &[sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]),
            sub(b"LVLO", [1u16.to_le_bytes().to_vec(), vec![0; 2], item.to_le_bytes().to_vec(), 1u16.to_le_bytes().to_vec(), vec![0; 2]].concat())]);
        let placed = |sig: &[u8; 4], id: u32, base: u32| record(sig, 0, id, &[sub(b"EDID", z(&format!("Ref{id:X}"))), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24]),
            sub(b"XMRC", 0x310u32.to_le_bytes().to_vec())]);
        let refs = [placed(b"ACHR", 0x301, 0x801), placed(b"ACHR", 0x302, 0x811), placed(b"ACRE", 0x303, 0x802), placed(b"ACRE", 0x304, 0x812),
            placed(b"REFR", 0x305, 0x820), placed(b"REFR", 0x310, 0x820)].concat();
        let cell = [record(b"CELL", 0, 0x200, &[sub(b"DATA", vec![1])]), group(0x200u32.to_le_bytes(), 6, &group(0x200u32.to_le_bytes(), 8, &refs))].concat();
        let bytes = [tes4(), top(b"NPC_", &[actor(b"NPC_", FormId::PLAYER_BASE.0), actor(b"NPC_", 0x801)].concat()), top(b"CREA", &actor(b"CREA", 0x802)),
            top(b"LVLN", &list(b"LVLN", 0x811, 0x801)), top(b"LVLC", &list(b"LVLC", 0x812, 0x802)),
            top(b"CONT", &record(b"CONT", 0, 0x820, &[sub(b"EDID", z("StockBox"))])),
            top(b"FACT", &record(b"FACT", 0, 0x830, &[sub(b"EDID", z("TestFaction"))])),
            top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell)))].concat();
        let plugins = [Plugin::parse(&bytes, "ActorQueries.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = Schema::load_dir(root.join("fnv")).unwrap();
        let path = root.join("script/functions.json");
        let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let forms = FormStore::build(&lo, &schema); let refs = RefTable::build(&lo, &schema);
        let mut data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
        data.package_defs.insert(FormId(0x840), PackageDef { target: Some(Target { kind: TargetKind::Reference(Some(FormId(0x305))), count_distance: 0 }), ..Default::default() });
        data.package_defs.insert(FormId(0x841), PackageDef { target: Some(Target { kind: TargetKind::LinkedReference, count_distance: 0 }), ..Default::default() });
        Fixture { data, forms, refs }
    }

    fn game(f: &Fixture) -> Game<'_> {
        Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() })
    }

    fn query(g: &mut Game, source: &str, receiver: Option<u32>) -> XVal {
        g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue ({source})\nEnd\n"), RunCtx { self_ref: receiver.map(FormId), ..Default::default() }).unwrap()
    }

    #[test]
    fn grab_query_reads_existing_grab_lifecycle_without_changing_it() {
        let f = fixture(); let mut g = game(&f);
        assert_eq!(query(&mut g, "GetPlayerGrabbedRef", None), XVal::Ref(0));
        g.set_player_grab(Some(FormId(0x305)));
        assert_eq!(query(&mut g, "GetPlayerGrabbedRef", None), XVal::Ref(0x305));
        assert_eq!(g.senses.grabbed, Some(FormId(0x305)));
        g.set_player_grab(None);
        assert_eq!(query(&mut g, "GetPlayerGrabbedRef", None), XVal::Ref(0));
    }

    #[test]
    fn package_target_uses_reference_and_linked_targets_for_ordinary_and_leveled_actors() {
        let f = fixture(); let mut g = game(&f);
        for actor in [0x301, 0x302, 0x303, 0x304] {
            g.extra.entry(FormId(actor)).or_default().current_package = Some(FormId(0x840));
            assert_eq!(query(&mut g, "GetPackageTarget", Some(actor)), XVal::Ref(0x305));
            g.extra.entry(FormId(actor)).or_default().current_package = Some(FormId(0x841));
            assert_eq!(query(&mut g, "GetPackageTarget", Some(actor)), XVal::Ref(0));
            g.jip2.links.insert(FormId(actor), FormId(0x310));
            assert_eq!(query(&mut g, "GetPackageTarget", Some(actor)), XVal::Ref(0x310));
        }
        for invalid in [None, Some(0), Some(0x305), Some(0xDEAD)] {
            assert_eq!(query(&mut g, "GetPackageTarget", invalid), XVal::Ref(0));
        }
    }

    #[test]
    fn package_target_keeps_omitted_self_and_explicit_empty_receiver_distinct_at_the_vm() {
        let f = fixture(); let mut g = game(&f);
        g.extra.entry(FormId(0x302)).or_default().current_package = Some(FormId(0x840));
        assert_eq!(query(&mut g, "GetPackageTarget", Some(0x302)), XVal::Ref(0x305));
        for statement in ["empty.GetPackageTarget", "SetFunctionValue (empty.GetPackageTarget)"] {
            let source = format!("Begin Function {{ }}\nref empty\n{statement}\nSetFactionRank TestFaction 50\nEnd\n");
            let error = g.run_query(&source, RunCtx { self_ref: Some(FormId(0x302)), ..Default::default() }).unwrap_err();
            assert_eq!(error, "getpackagetarget called on no reference");
            assert_eq!(crate::functions::actor::faction_rank(&g, FormId(0x302), FormId(0x830)), -1);
            assert_eq!(g.extra[&FormId(0x302)].current_package, Some(FormId(0x840)));
        }
    }

    #[test]
    fn player_conversation_partner_precedes_the_actors_package_target() {
        let f = fixture(); let mut g = game(&f);
        g.extra.entry(FormId(0x302)).or_default().current_package = Some(FormId(0x840));
        g.dialogue = Some(crate::dialogue::Conversation { actor: FormId(0x302), through: None, line: None, choices: vec![], topic_list: false, lines: 0, then: None, ending: false });
        assert_eq!(query(&mut g, "GetPackageTarget", Some(0x302)), XVal::Ref(FormId::PLAYER_REF.0));
        assert_eq!(query(&mut g, "GetPackageTarget", Some(FormId::PLAYER_REF.0)), XVal::Ref(0x302));
        g.dialogue = None;
        assert_eq!(query(&mut g, "GetPackageTarget", Some(0x302)), XVal::Ref(0x305));
    }

    #[test]
    fn actor_conversation_returns_the_other_participant_from_either_side() {
        let f = fixture(); let mut g = game(&f);
        g.speech.push(crate::game::Speech { speaker: FormId(0x301), target: Some(FormId(0x303)), topic: FormId(0), info: FormId(0), remaining: 1.0,
            say_to: false, greeting: false, farewell: false, conversation: Some(1), earlier_topics: vec![], hello: false,
            look: crate::game::LineLook::Both, response: 0, force_subtitles: false, two_d: false, end_ran: false, remote: false, leads_on: false, through: None });
        assert_eq!(query(&mut g, "GetPackageTarget", Some(0x301)), XVal::Ref(0x303));
        assert_eq!(query(&mut g, "GetPackageTarget", Some(0x303)), XVal::Ref(0x301));
    }

    #[test]
    fn merchant_clear_survives_save_and_later_setter_replaces_it() {
        let f = fixture(); let mut g = game(&f);
        for actor in [0x301, 0x302, 0x303, 0x304] {
            assert_eq!(g.merchant_container(FormId(actor)), Some(FormId(0x310)));
            let (unit, _) = g.compile_statements(&format!("RemoveMerchantContainer Ref{actor:X}\n"), "ActorQueries.esm", "SCPT").unwrap();
            assert!(g.run_statements(unit, Default::default()));
            assert_eq!(g.merchant_container(FormId(actor)), None);
        }
        assert_eq!(call(&mut g, "removemerchantcontainer", Some(FormId(0x301)), &[XVal::Ref(0)]).unwrap().unwrap(), XVal::Num(0.0));
        assert!(!g.extra.contains_key(&FormId(0)));
        let bytes = serde_json::to_vec(&g.checked_snapshot().unwrap()).unwrap();
        let saved: crate::game::GameSnapshot = serde_json::from_slice(&bytes).unwrap();
        let world = World::restore(&f.forms, &f.refs, &saved.sim).unwrap();
        let mut back = Game::restore(&f.data, world, g.cfg, &saved).unwrap();
        assert_eq!(back.merchant_container(FormId(0x302)), None);
        back.run_query("Begin Function { }\nRef302.SetMerchantContainer Ref310\nEnd\n", Default::default()).unwrap();
        assert_eq!(back.merchant_container(FormId(0x302)), Some(FormId(0x310)));
    }

    #[test]
    fn removefac_alias_uses_the_existing_reference_faction_handler() {
        let f = fixture(); let mut g = game(&f);
        assert_eq!(f.data.table.by_name("RemoveFac"), f.data.table.by_name("RemoveFromFaction"));
        g.run_query("Begin Function { }\nRef301.SetFactionRank TestFaction 2\nRef301.RemoveFac TestFaction\nEnd\n", Default::default()).unwrap();
        assert_eq!(crate::functions::actor::faction_rank(&g, FormId(0x301), FormId(0x830)), -1);
    }

    #[test]
    fn detection_query_admits_selected_leveled_actors_without_changing_cache_rules() {
        use crate::detection::{Band, Detection};
        let f = fixture(); let mut g = game(&f);
        for target in [0x301, 0x302, 0x303, 0x304] {
            let detector = if target == 0x301 { 0x303 } else { 0x301 };
            g.detection.record(FormId(detector), FormId(target), Detection { value: 10.0, band: Band::Detected, ..Default::default() }, g.time);
            let before = g.detection.pairs.len();
            assert_eq!(query(&mut g, "IsActorDetected", Some(target)), XVal::Num(1.0));
            assert_eq!(g.detection.pairs.len(), before);
        }
        g.detection.pairs.clear();
        for detector in [FormId::PLAYER_REF, FormId(0x302)] {
            g.detection.record(detector, FormId(0x302), Detection { value: 10.0, band: Band::Detected, ..Default::default() }, g.time);
        }
        assert_eq!(query(&mut g, "IsActorDetected", Some(0x302)), XVal::Num(0.0));
        g.detection.record(FormId(0x301), FormId(0x305), Detection { value: 10.0, band: Band::Detected, ..Default::default() }, g.time);
        assert_eq!(query(&mut g, "IsActorDetected", Some(0x305)), XVal::Num(0.0));
    }
}
