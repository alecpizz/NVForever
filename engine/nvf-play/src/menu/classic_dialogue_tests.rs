//! Classic dialogue through the real barter request/open systems, with authored data.
use super::*;
use fnv_esm::{raw::{write_subrecords, Subrecord}, Plugin};

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord { Subrecord { sig: *sig, data, extended: false } }
fn z(text: &str) -> Vec<u8> { [text.as_bytes(), &[0]].concat() }
fn record(sig: &[u8; 4], id: u32, subs: &[Subrecord]) -> Vec<u8> {
    let body = write_subrecords(subs);
    [&sig[..], &(body.len() as u32).to_le_bytes(), &[0; 4], &id.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &body].concat()
}
fn group(label: [u8; 4], kind: i32, body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &kind.to_le_bytes(), &[0; 8], body].concat()
}
fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> { group(*sig, 0, body) }

fn fixture() -> Vec<u8> {
    let inventory = |item: u32| sub(b"CNTO", [item.to_le_bytes(), 1i32.to_le_bytes()].concat());
    let actor = |sig, id| record(sig, id, &[inventory(0x901)]);
    let list = |sig, id, actor: u32| record(sig, id, &[
        sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]),
        sub(b"LVLO", [1u16.to_le_bytes().to_vec(), vec![0; 2], actor.to_le_bytes().to_vec(), 1u16.to_le_bytes().to_vec(), vec![0; 2]].concat()),
    ]);
    let placed = |sig, id: u32, base: u32| {
        let mut fields = vec![
            sub(b"EDID", z(&format!("Ref{id:X}"))),
            sub(b"NAME", base.to_le_bytes().to_vec()),
            sub(b"DATA", vec![0; 24]),
        ];
        if id == 0x301 {
            fields.push(sub(b"XMRC", 0x310u32.to_le_bytes().to_vec()));
        }
        record(sig, id, &fields)
    };
    let refs = [placed(b"ACHR", 0x301, 0x801), placed(b"ACHR", 0x302, 0x811),
        placed(b"ACRE", 0x303, 0x802), placed(b"ACRE", 0x304, 0x812),
        placed(b"REFR", 0x310, 0x820), placed(b"REFR", 0x311, 0x821)].concat();
    let cell = [record(b"CELL", 0x200, &[sub(b"DATA", vec![1])]), group(0x200u32.to_le_bytes(), 6, &group(0x200u32.to_le_bytes(), 8, &refs))].concat();
    let item = |id: u32| record(b"MISC", id, &[sub(b"EDID", z(&format!("Item{id:X}"))), sub(b"DATA", [5i32.to_le_bytes(), 1f32.to_le_bytes()].concat())]);
    [record(b"TES4", 0, &[sub(b"HEDR", [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat())]),
        top(b"NPC_", &[actor(b"NPC_", FormId::PLAYER_BASE.0), actor(b"NPC_", 0x801)].concat()),
        top(b"CREA", &actor(b"CREA", 0x802)), top(b"LVLN", &list(b"LVLN", 0x811, 0x801)), top(b"LVLC", &list(b"LVLC", 0x812, 0x802)),
        top(b"CONT", &[record(b"CONT", 0x820, &[inventory(0x902)]), record(b"CONT", 0x821, &[inventory(0x903)])].concat()),
        top(b"MISC", &[item(0x901), item(0x902), item(0x903)].concat()),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ].concat()
}

pub(crate) fn classic_merchant_world() -> (crate::script::ScriptState, SimState) {
    let mut bytes = fixture();
    bytes.extend(top(b"MISC", &record(b"MISC", model::CAPS.0, &[sub(b"EDID", z("Caps")), sub(b"DATA", [1i32.to_le_bytes(), 0f32.to_le_bytes()].concat())])));
    let topic = 0xa01u32;
    let info = 0xa02u32;
    let info_record = record(b"INFO", info, &[sub(b"TRDT", vec![0; 24]), sub(b"NAM1", z("{smiles} Welcome back."))]);
    bytes.extend(top(b"DIAL", &[record(b"DIAL", topic, &[]), group(topic.to_le_bytes(), 7, &info_record)].concat()));
    let plugins = [Plugin::parse(&bytes, "ClassicMerchant.esm".into()).unwrap()];
    let mut script = crate::script::ScriptState::over_plugins(&plugins);
    let rt = script.rt.as_mut().unwrap();
    let forms = rt.game.world.forms;
    let refs = rt.game.world.placed;
    let sim = SimState { forms, refs, world: std::mem::replace(&mut rt.game.world, fnv_sim::World::new(forms, refs, Default::default())) };
    (script, sim)
}

#[test]
fn classic_barter_opens_the_vendor_stock_and_cancel_resumes_the_same_choices() {
    use crate::dialogue::{list_id, observe_classic};
    use crate::dialogue_touch::classic_state::{request, State as Classic};
    use fnv_game::dialogue::{Choice, Conversation, Line};

    let (mut script, sim) = classic_merchant_world();
    let rt = script.rt.as_mut().unwrap();
    let topic = 0xa01;
    let info = 0xa02;
    let merchant = FormId(0x301);
    let choice = Choice { topic: FormId(topic), info: FormId(info), chars: 1, prompt: false, goodbye: false, darkened: false, challenge: None, dumb: false };
    rt.game.dialogue = Some(Conversation {
        actor: merchant, through: None,
        line: Some(Line { topic: FormId(topic), info: FormId(info), response: 0, remaining: 1.0 }),
        choices: vec![choice], topic_list: true, lines: 1, then: None, ending: false,
    });
    let mut classic = Classic::default();
    observe_classic(&mut classic, &rt.game);
    assert_eq!(classic.spoken.as_deref(), Some("Welcome back."));
    assert!(classic.speaking);
    let spoken_list = list_id(&rt.game);
    request(&mut rt.game, spoken_list.as_ref());
    assert!(rt.game.barter_requests.is_empty());
    rt.game.dialogue.as_mut().unwrap().line = None;
    observe_classic(&mut classic, &rt.game);
    assert_eq!(classic.spoken.as_deref(), Some("Welcome back."));
    assert!(!classic.speaking);

    let mut app = App::new();
    app.insert_resource(script).insert_resource(sim)
        .insert_resource(State::new(PlayState::Dialogue)).init_resource::<NextState<PlayState>>()
        .init_resource::<ActiveMenu>().init_resource::<FreshMenu>().init_resource::<PromptKeys>()
        .add_message::<OpenContainer>().add_message::<crate::pickpocket::OpenPickpocket>()
        .add_message::<OpenInventory>().add_message::<OpenBarter>().add_message::<MenuInput>()
        .add_message::<RefChanged>().add_message::<ItemUsed>().add_message::<crate::ui_sounds::UiSound>()
        .add_message::<crate::crime::ItemsTaken>().add_message::<crate::pickpocket::Pickpocketed>()
        .add_systems(Update, (scripts_open_barter, open_menus, apply_input).chain());
    use crate::dialogue_touch::appearance::{Appearance, Theme};
    use crate::dialogue_touch::classic_state::refresh_fixture;
    app.insert_resource(classic).insert_resource(Appearance { theme: Theme::ClassicGlass, ..Default::default() });
    app.world_mut().resource_mut::<SimState>().world.add_item(FormId::PLAYER_REF, model::CAPS, 11).unwrap();
    refresh_fixture(app.world_mut());
    assert_eq!(app.world().resource::<Classic>().caps, Some(11));
    assert_eq!(app.world().resource::<Classic>().barter.as_ref().unwrap().merchant, merchant.0);
    let offered = app.world_mut().resource_scope(|world, mut script: Mut<crate::script::ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        crate::script::with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            let g = &mut rt.game;
            request(g, spoken_list.as_ref());
            assert!(g.barter_requests.is_empty());
            let list = list_id(g).unwrap();
            g.extra.entry(merchant).or_default().dead = true;
            request(g, Some(&list));
            assert!(g.barter_requests.is_empty());
            g.extra.entry(merchant).or_default().dead = false;
            g.dialogue.as_mut().unwrap().actor = FormId(0x302);
            let non_merchant_list = list_id(g).unwrap();
            request(g, Some(&non_merchant_list));
            assert!(g.barter_requests.is_empty());
            g.dialogue.as_mut().unwrap().actor = merchant;
            request(g, Some(&list));
            request(g, Some(&list));
            assert_eq!(g.barter_requests.len(), 1);
            list
        })
    });
    app.update();
    let menu = app.world().resource::<ActiveMenu>().0.as_ref().unwrap();
    assert_eq!(menu.mode, Mode::Barter(merchant));
    assert_eq!(menu.trade.as_ref().unwrap().stock, FormId(0x310));
    app.world_mut().write_message(MenuInput(Command::Close));
    app.update();
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    refresh_fixture(app.world_mut());
    assert_eq!(app.world().resource::<Classic>().caps, Some(11));
    app.world_mut().resource_mut::<SimState>().world.add_item(FormId::PLAYER_REF, model::CAPS, 4).unwrap();
    refresh_fixture(app.world_mut());
    assert_eq!(app.world().resource::<Classic>().caps, Some(15));
    app.insert_resource(SimState::over_placed([]));
    refresh_fixture(app.world_mut());
    assert_eq!(app.world().resource::<Classic>().caps, None);
    let mut classic = app.world_mut().remove_resource::<Classic>().unwrap();
    let mut script = app.world_mut().resource_mut::<crate::script::ScriptState>();
    let g = &mut script.rt.as_mut().unwrap().game;
    assert!(g.menu.is_none());
    assert_eq!(list_id(g).as_ref(), Some(&offered));
    observe_classic(&mut classic, g);
    assert_eq!(classic.spoken.as_deref(), Some("Welcome back."));
    g.dialogue_serial += 1;
    observe_classic(&mut classic, g);
    assert!(classic.spoken.is_none());
    g.dialogue.as_mut().unwrap().line = Some(Line { topic: FormId(topic), info: FormId(info), response: 0, remaining: 1.0 });
    observe_classic(&mut classic, g);
    assert_eq!(classic.spoken.as_deref(), Some("Welcome back."));
    let conversation = g.dialogue.as_mut().unwrap();
    conversation.actor = FormId(0x302);
    conversation.line = None;
    observe_classic(&mut classic, g);
    assert!(classic.spoken.is_none());
    g.dialogue = None;
    observe_classic(&mut classic, g);
    assert!(classic.barter.is_none() && classic.caps.is_none() && !classic.speaking);
}

#[test]
fn classic_raw_trade_release_opens_barter_and_a_revoked_held_target_stays_closed() {
    use bevy::input::touch::{TouchInput, TouchPhase};
    use crate::dialogue_touch::classic_state::State as Classic;
    use fnv_game::dialogue::{Choice, Conversation};

    crate::dialogue_touch::classic_presenter::with_barter_surface(|mut app, window| {
        let (mut script, mut sim) = classic_merchant_world();
        let merchant = FormId(0x301);
        let rt = script.rt.as_mut().unwrap();
        rt.game.dialogue = Some(Conversation {
            actor: merchant, through: None, line: None,
            choices: vec![Choice { topic: FormId(0xa01), info: FormId(0xa02), chars: 1,
                prompt: false, goodbye: false, darkened: false, challenge: None, dumb: false }],
            topic_list: true, lines: 1, then: None, ending: false,
        });
        let list = crate::dialogue::list_id(&rt.game).unwrap();
        sim.world.add_item(FormId::PLAYER_REF, model::CAPS, 19).unwrap();
        let mut phys = crate::phys::PhysState::default();
        let _ = phys.place_player([0.; 3], 48., 400.);
        app.insert_resource(script).insert_resource(sim).insert_resource(phys)
            .init_resource::<ActiveMenu>().init_resource::<FreshMenu>().init_resource::<PromptKeys>()
            .add_message::<OpenContainer>().add_message::<crate::pickpocket::OpenPickpocket>()
            .add_message::<OpenInventory>().add_message::<OpenBarter>().add_message::<MenuInput>()
            .add_message::<RefChanged>().add_message::<ItemUsed>().add_message::<crate::ui_sounds::UiSound>()
            .add_message::<crate::crime::ItemsTaken>().add_message::<crate::pickpocket::Pickpocketed>()
            .add_systems(Update, (scripts_open_barter, open_menus, apply_input).chain()
                .in_set(crate::PlaySet::Ui).before(crate::dialogue_menu::DrawMenu));
        crate::dialogue::install_classic_input_fixture(&mut app);
        let settle = |app: &mut App| { for _ in 0..6 { app.update(); } };
        let button = |app: &mut App| {
            let mut query = app.world_mut().query::<(&Name, &ComputedNode, &UiGlobalTransform)>();
            let mut buttons = query.iter(app.world()).filter(|(name, _, _)| name.as_str() == "Classic Trade button");
            let (_, node, transform) = buttons.next().expect("Classic painted the enabled Trade button");
            assert!(buttons.next().is_none(), "one Trade target");
            assert!(node.size().min_element() >= 47.99);
            transform.transform_point2(Vec2::ZERO)
        };
        let event = |app: &mut App, id, phase, position| {
            app.world_mut().write_message(TouchInput { window, id, phase, position, force: None });
            app.update();
        };
        settle(&mut app);
        let point = button(&mut app);
        event(&mut app, 81, TouchPhase::Started, point);
        assert!(app.world().resource::<ActiveMenu>().0.is_none());
        {
            let mut script = app.world_mut().resource_mut::<crate::script::ScriptState>();
            script.rt.as_mut().unwrap().game.extra.entry(merchant).or_default().dead = true;
        }
        event(&mut app, 81, TouchPhase::Ended, point);
        assert!(app.world().resource::<ActiveMenu>().0.is_none());
        assert!(app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.menu.is_none());
        {
            let mut script = app.world_mut().resource_mut::<crate::script::ScriptState>();
            script.rt.as_mut().unwrap().game.extra.entry(merchant).or_default().dead = false;
        }
        settle(&mut app);
        let point = button(&mut app);
        event(&mut app, 82, TouchPhase::Started, point);
        assert!(app.world().resource::<ActiveMenu>().0.is_none());
        event(&mut app, 82, TouchPhase::Ended, point);
        let m = app.world().resource::<ActiveMenu>().0.as_ref().expect("release opens the normal barter menu");
        assert_eq!(m.mode, Mode::Barter(merchant));
        assert_eq!(m.trade.as_ref().unwrap().stock, FormId(0x310));
        app.world_mut().write_message(MenuInput(Command::Close));
        settle(&mut app);
        assert!(app.world().resource::<ActiveMenu>().0.is_none());
        let script = app.world().resource::<crate::script::ScriptState>();
        let g = &script.rt.as_ref().unwrap().game;
        assert_eq!(crate::dialogue::list_id(g), Some(list));
        assert!(g.menu.is_none() && g.barter_requests.is_empty());
        assert_eq!(app.world().resource::<Classic>().caps, Some(19));
        button(&mut app);
    });
}
