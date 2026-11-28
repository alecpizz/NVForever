//! Dialogue checks against the game's own masters (docs/spec-notes/dialogue.md §2, §9).
//! They need the game data, which is never in the repository: set `FNVLAB_DATA_DIR` (the
//! parity lab's `data_dir`, parity-lab/lab.json) to the game's `Data` folder. Without it,
//! or without the ten masters there, each test prints why it was skipped and passes.

use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::data::{info_flag, topic_type};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

const MASTERS: [&str; 10] = [
    "FalloutNV.esm",
    "DeadMoney.esm",
    "HonestHearts.esm",
    "OldWorldBlues.esm",
    "LonesomeRoad.esm",
    "GunRunnersArsenal.esm",
    "ClassicPack.esm",
    "MercenaryPack.esm",
    "TribalPack.esm",
    "CaravanPack.esm",
];

/// The game's Data folder, or `None` (with the reason printed) when the checks cannot run.
fn data_dir() -> Option<PathBuf> {
    let Some(dir) = std::env::var_os("FNVLAB_DATA_DIR").map(PathBuf::from) else {
        eprintln!("skipped: FNVLAB_DATA_DIR is not set (no game data)");
        return None;
    };
    if let Some(missing) = MASTERS.iter().find(|m| !dir.join(m).is_file()) {
        eprintln!("skipped: {missing} is not in {}", dir.display());
        return None;
    }
    Some(dir)
}

struct Loaded {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn load(dir: &Path) -> Loaded {
    let plugins: Vec<Plugin> = MASTERS.iter().map(|m| Plugin::open(dir.join(m)).unwrap()).collect();
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = Schema::load_dir(root.join("schemas/fnv")).unwrap();
    let fp = root.join("schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&fp).unwrap()).unwrap();
    let table = FunctionTable::load(&fp).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Loaded { data, forms, refs }
}

/// One load for all checks (the build takes a few seconds).
#[test]
fn the_masters_dialogue() {
    let Some(dir) = data_dir() else { return };
    let mut l = load(&dir);
    counts(&l.data);
    doc_mitchells_lines(&l.data);
    the_intro_walk_through_talks_its_way_to_the_end(&l);
    sunny_opens_conversations_with_conversation_lines(&l);
    a_dim_player_gets_the_dumb_responses_and_lines(&l);
    powder_ganger_hellos_keep_their_shared_random_pool(&l);
    vms07_shared_greetings_remain_selectable_with_controlled_eligibility(&mut l);
}

/// [observed: fnvtool dialogue topics/lines over FalloutNV.esm, issue #1895] The eligible
/// pool and actor references come from the issue's data survey. Per-listing-quest insertion restores that pool in HELLO
/// (see docs/spec-notes/dialogue.md §1.2); this does not establish original-game behavior.
fn powder_ganger_hellos_keep_their_shared_random_pool(l: &Loaded) {
    use fnv_game::game::{Game, GameConfig};
    use fnv_sim::{SimConfig, World};
    let hello = l.data.by_edid("HELLO").unwrap();
    let pool = [0x0015_1A7B, 0x0015_1A7A, 0x0015_694C, 0x0015_694D].map(FormId);
    let plain = FormId(0x0015_1A7C);
    let lines = l.data.lines_tried(hello);
    let positions = pool.map(|i| lines.iter().position(|&line| line == i).unwrap());
    assert!(positions.windows(2).all(|p| p[0] < p[1]), "{positions:?}");
    let plain_at = lines.iter().position(|&line| line == plain).unwrap();
    assert!(positions.iter().all(|&at| at < plain_at));
    // Other conditioned Random lines may be between the four eligible lines; a plain
    // line or an earlier Random End would split their selection pool.
    assert!(lines[positions[0]..=positions[3]].iter().all(|i| l.data.infos[i].has(info_flag::RANDOM)));
    assert!(!lines[positions[0]..positions[3]].iter().any(|i| l.data.infos[i].has(info_flag::RANDOM_END)));
    assert!(!l.data.infos[&plain].has(info_flag::RANDOM));
    for actor in [FormId(0x000F_0AF8), FormId(0x000F_0AF9)] {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..16 {
            let world = World::new(&l.forms, &l.refs, SimConfig::default());
            let mut g = Game::new(&l.data, world, GameConfig { seed, ..GameConfig::default() });
            g.new_game();
            for &line in &pool {
                assert!(g.can_say(actor, Some(FormId::PLAYER_REF), line), "{actor}: {line}");
            }
            for _ in 0..16 {
                let line = g.say_hello(actor).expect("a Powder Ganger hello");
                assert!(pool.contains(&line), "{actor}, seed {seed}: {line}");
                seen.insert(line);
                // This checks selection repeatedly without advancing quest scripts or
                // requiring voice assets / the residents' approach timer.
                g.speech.clear();
            }
        }
        assert_eq!(seen, pool.into_iter().collect());
    }
}

/// [design] Exercise the real GREETING list with controlled eligibility, independently
/// of VMS07's quest flow: only the target INFO can pass the speaker/condition gates.
/// The original ordering, ownership, indices and Random flags are retained. This is
/// selection coverage, not a reproduction of the quest's original-game conditions.
fn vms07_shared_greetings_remain_selectable_with_controlled_eligibility(l: &mut Loaded) {
    use fnv_game::game::{Game, GameConfig};
    use fnv_sim::{SimConfig, World};
    let greeting = l.data.greeting.unwrap();
    let source = l.data.by_edid("VMS07Greeting").unwrap();
    let shared = [0x000F_378E, 0x000F_378F, 0x000F_3791, 0x000F_479B].map(FormId);
    assert!(shared.iter().all(|i| l.data.infos[i].topic == Some(source)));
    for def in l.data.infos.values_mut() {
        def.speaker = Some(FormId(u32::MAX));
        // GetIsID normally overrides the speaker field; isolate that gate too.
        def.conditions.clear();
    }
    for info in shared {
        let quest = l.data.infos[&info].quest.unwrap();
        l.data.quests.get_mut(&quest).unwrap().conditions.clear();
        let def = l.data.infos.get_mut(&info).unwrap();
        def.speaker = Some(FormId::PLAYER_REF);
        def.flags &= !(info_flag::LOW_INTELLIGENCE | info_flag::HIGH_INTELLIGENCE);
        let world = World::new(&l.forms, &l.refs, SimConfig::default());
        let mut g = Game::new(&l.data, world, GameConfig::default());
        // Do not run new-game result scripts while eligibility is isolated.
        g.quest(quest).running = true;
        assert!(g.can_say(FormId::PLAYER_REF, Some(FormId::PLAYER_REF), info));
        assert_eq!(g.select_line(FormId::PLAYER_REF, Some(FormId::PLAYER_REF), greeting), Some(info));
        drop(g);
        l.data.infos.get_mut(&info).unwrap().speaker = Some(FormId(u32::MAX));
    }
}

/// The player's Intelligence against iDialogueDummySpeakThisIntOrBelow (3): the new game's
/// 5 is not dim, 2 is; a Low Intelligence line is said to a dim player only, a High
/// Intelligence one only to one who is not (`Game::player_dim`).
fn a_dim_player_gets_the_dumb_responses_and_lines(l: &Loaded) {
    use fnv_game::game::{Game, GameConfig};
    use fnv_sim::{SimConfig, World};
    let world = World::new(&l.forms, &l.refs, SimConfig::default());
    let mut g = Game::new(&l.data, world, GameConfig::default());
    g.new_game();
    assert!(!g.player_dim());
    let int = fnv_game::actor_values::INTELLIGENCE;
    let five = fnv_game::functions::actor_value_capped(&mut g, FormId::PLAYER_REF, int);
    assert_eq!(five, 5.0);
    let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode: 0x100F, on: Some(FormId::PLAYER_REF), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[fnv_script::vm::ArgValue::Enum(int), fnv_script::vm::ArgValue::Value(fnv_script::vm::Value::Num(2.0))]);
    assert!(g.player_dim());
    // 135 topics carry a Dumb Response (observed; dialogue.md §1).
    assert_eq!(l.data.topic_defs.values().filter(|t| t.dumb_response.is_some()).count(), 135);
}

/// Sunny Smiles opening a conversation with each Goodsprings resident takes a
/// Conversation line of HELLO, never a Topic line for the player (dialogue.md §5.1,
/// `fnv_game::dialogue::between_actors`) [owner report 2026-10-11: in a test run Sunny
/// greeted another actor with the player's lines]. The selection without the rule, kept
/// here for comparison, takes Topic lines for most of them.
fn sunny_opens_conversations_with_conversation_lines(l: &Loaded) {
    use fnv_game::game::{Game, GameConfig};
    use fnv_sim::{SimConfig, World};
    let world = World::new(&l.forms, &l.refs, SimConfig::default());
    let mut g = Game::new(&l.data, world, GameConfig::default());
    g.new_game();
    let sunny = l.data.by_edid("SunnyRef").expect("SunnyRef");
    let hello = g.hello_topic().expect("HELLO");
    let names = [
        "TrudyRef", "GSJoeCobbRef", "EasyPeteRef", "GSChetRef", "DocMitchellREF", "RingoRef", "GSVictorRef",
        "GoodspringsSettler01Ref", "GoodspringsSettler02Ref", "GoodspringsSettler03Ref", "GoodspringsSettler04Ref",
    ];
    let (mut topic_before, mut partners, mut opened) = (0, 0, 0);
    for name in names {
        let Some(other) = l.data.by_edid(name) else { continue };
        partners += 1;
        // Before the rule: any HELLO line Sunny can say to this actor.
        if let Some(i) = g.select_line(sunny, Some(other), hello) {
            if l.data.infos[&i].info_type == topic_type::TOPIC {
                topic_before += 1;
            }
        }
        if let Some(i) = g.conversation_opener(sunny, other) {
            opened += 1;
            let d = &l.data.infos[&i];
            assert_eq!(d.info_type, topic_type::CONVERSATION, "Sunny to {name}: line {i} is not a Conversation line");
            assert!(d.link_from.is_empty(), "Sunny to {name}: line {i} has a Link From");
        }
    }
    eprintln!("Sunny's HELLO to {partners} Goodsprings residents: {topic_before} would have been Topic lines without the rule; now {opened} open with a Conversation line, none with a Topic line");
    assert!(partners >= 8, "the Goodsprings residents are in the masters");
}

/// The counts of docs/spec-notes/dialogue.md §2 [observed: these masters].
fn counts(d: &GameData) {
    assert_eq!((d.topic_defs.len(), d.infos.len()), (21_298, 28_896));
    let greeting = d.greeting.expect("GREETING");
    assert_eq!((d.edid_of[&greeting].as_str(), d.topics[&greeting].len()), ("GREETING", 5_579));
    assert_eq!(d.edid_of[&d.goodbye.expect("GOODBYE")], "GOODBYE");
    // Previous INFO (PNAM) moves no line of the ten masters out of file order (#968).
    assert_eq!(d.lines_moved_by_previous, 0);
    assert_eq!(d.top_level.len(), 2_536);
    assert_eq!(d.topic_defs.values().filter(|t| t.topic_type == topic_type::TOPIC).count(), 19_285);
    // Every line sits under a topic and names a quest; every topic of a line is read.
    assert!(d.infos.values().all(|i| i.quest.is_some() && i.topic.is_some_and(|t| d.topic_defs.contains_key(&t))));
    let with = |flag: u16| d.infos.values().filter(|i| i.has(flag)).count();
    assert_eq!((with(info_flag::GOODBYE), with(info_flag::SAY_ONCE), with(info_flag::RANDOM)), (9_828, 4_434, 8_001));
    assert_eq!((with(info_flag::RUN_IMMEDIATELY), with(info_flag::SAY_ONCE_A_DAY), with(info_flag::SPEECH_CHALLENGE)), (765, 194, 3));
    assert_eq!(d.infos.values().map(|i| i.responses.len()).sum::<usize>(), 37_403);
    assert!(d.infos.values().all(|i| i.responses.len() == i.response_chars.len()));
    assert_eq!(d.infos.values().filter(|i| !i.choices.is_empty()).count(), 8_114);
    assert_eq!(d.infos.values().filter(|i| !i.follow_ups.is_empty()).count(), 1_759);
    // Shared lines (dialogue.md §1.2): player lines with choices, and those whose choices
    // all lack lines, counting a topic's own lines only and then its shared ones too.
    let player: Vec<_> = d.infos.values().filter(|i| i.info_type == topic_type::TOPIC && !i.choices.is_empty()).collect();
    let dead = |lines: &dyn Fn(FormId) -> Vec<FormId>| player.iter().filter(|i| i.choices.iter().all(|&c| lines(c).is_empty())).count();
    assert_eq!((player.len(), dead(&|t| d.lines_by_priority(t)), dead(&|t| d.lines_tried(t))), (7_325, 257, 7));
    // Red Lucy's "What's this place?" has no line of its own and shares the answer of
    // the topic of the same name.
    let lucy = d.by_edid("VMS38VMS38RedLucyTopic047").unwrap();
    assert_eq!((d.lines_by_priority(lucy), d.lines_tried(lucy)), (vec![], vec![FormId(0x0013_4FC1)]));
    assert_eq!(d.infos[&FormId(0x0013_4FC1)].topic, d.by_edid("VMS38VMS38RedLucyTopic004"));
    // Link From turned round (dialogue.md §3.5), by the linked topic's type, `ANY` (no
    // lines) left out: (named topics, links, links not already a choice of every line of
    // the named topic).
    let pairs: Vec<(FormId, FormId)> = d.linked_from.iter().flat_map(|(x, ts)| ts.iter().map(move |t| (*x, *t))).collect();
    let any = d.by_edid("ANY").unwrap();
    let of_type = |want: u8| {
        let p: Vec<&(FormId, FormId)> = pairs.iter().filter(|(x, t)| d.topic_defs[t].topic_type == want && *x != any).collect();
        let named: std::collections::BTreeSet<FormId> = p.iter().map(|(x, _)| *x).collect();
        let new = p.iter().filter(|(x, t)| !d.lines_tried(*x).iter().all(|l| d.infos[l].choices.contains(t))).count();
        (named.len(), p.len(), new)
    };
    assert_eq!(of_type(topic_type::TOPIC), (58, 185, 19));
    assert_eq!(of_type(topic_type::CONVERSATION), (65, 83, 32));
    assert_eq!(of_type(topic_type::RADIO), (21, 54, 50));
    assert_eq!((d.linked_from.len(), pairs.len(), d.linked_from[&any].len()), (150, 354, 26));
    assert!(d.lines_tried(any).is_empty());
    let ma = |n: &str| d.by_edid(&format!("VMS22MichaelAngeloTopic{n}")).unwrap();
    assert_eq!(d.linked_from[&greeting], vec![ma("003"), ma("004")]);
    // Michael Angelo's "Thank you for the offer…" lists 035, 042 and 004; the lines of
    // 031, 032 and 033 name it in their Link From.
    assert_eq!(d.infos[&FormId(0x0011_3C4E)].choices, vec![ma("035"), ma("042"), ma("004")]);
    assert!(["031", "032", "033"].iter().all(|n| d.linked_from[&ma("034")].contains(&ma(n))));
    // GREETING shares five lines; four of VMS07Greeting go in at indices 11–14.
    let greeting_tried = d.lines_tried(greeting);
    assert_eq!(greeting_tried.len(), 5_579 + 5);
    let shared = [0x000F_378E, 0x000F_378F, 0x000F_3791, 0x000F_479B].map(FormId);
    let listing_quest = d.topic_defs[&greeting].shared_infos.iter().find(|s| s.info == Some(shared[0])).unwrap().quest.unwrap();
    let partition: Vec<_> = d.shared_lines[&greeting].iter().copied()
        .filter(|i| d.infos[i].quest == Some(listing_quest) || shared.contains(i)).collect();
    let authored = d.topics[&greeting].iter().filter(|i| d.infos[i].quest == Some(listing_quest)).count();
    let at = 11.min(authored); // Past the quest's own lines: after its last one.
    assert_eq!(partition[at..at + 4], shared);
    // Shared lines go in among the own lines, which keep the order lines_by_priority
    // tries them in (dialogue.md §1.2, §4).
    for topic in [greeting, d.by_edid("HELLO").unwrap()] {
        let own: std::collections::HashSet<FormId> = d.topics[&topic].iter().copied().collect();
        let own_tried: Vec<FormId> = d.lines_tried(topic).into_iter().filter(|i| own.contains(i)).collect();
        assert_eq!(own_tried, d.lines_by_priority(topic), "{topic}");
    }
    assert_eq!(d.infos.values().filter(|i| i.challenge_form.is_some()).count(), 1_499);
    // Quests whose own conditions gate their lines (dialogue.md §4).
    let gated: Vec<FormId> = d.quests.values().filter(|q| !q.conditions.is_empty()).map(|q| q.form).collect();
    assert_eq!((d.quests.len(), gated.len()), (640, 113));
    assert_eq!(d.infos.values().filter(|i| i.quest.is_some_and(|q| gated.contains(&q))).count(), 8_151);
    // Run Immediately is set on Radio and Conversation lines only.
    assert!(d.infos.values().filter(|i| i.has(info_flag::RUN_IMMEDIATELY)).all(|i| i.info_type == topic_type::RADIO || i.info_type == topic_type::CONVERSATION));
    // Links point at records of the documented kinds.
    for i in d.infos.values() {
        assert!(i.choices.iter().chain(&i.add_topics).chain(&i.link_from).all(|t| d.topic_defs.contains_key(t)));
        assert!(i.follow_ups.iter().all(|f| d.infos.contains_key(f)));
    }
}

/// Doc Mitchell's psych test: the greeting that opens it offers six topics, and each
/// answer leads on [observed: FalloutNV.esm].
fn doc_mitchells_lines(d: &GameData) {
    let open = &d.infos[&FormId(0x0010_558F)];
    assert_eq!(open.topic, d.greeting);
    assert!(open.say_once && open.choices.len() == 6 && open.begin.is_some() && open.end.is_some());
    let farewell = &d.infos[&FormId(0x0010_57E8)];
    assert_eq!(farewell.follow_ups, vec![FormId(0x0010_57E4)]);
    assert_eq!(d.infos[&FormId(0x0010_57E4)].follow_ups.len(), 2);
}

/// The headless intro (docs/spec-notes/scripts.md §9) with the psych test and Doc's
/// farewell as conversations: no step fails and VCG01 ends at stage 200.
fn the_intro_walk_through_talks_its_way_to_the_end(l: &Loaded) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("walkthrough/intro.actions.json");
    let input: fnv_game::runner::Input = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let t = fnv_game::runner::run(&l.data, &l.forms, &l.refs, &input).unwrap();
    let failed: Vec<&Json> = t["steps"].as_array().unwrap().iter().filter(|s| !s["error"].is_null()).collect();
    assert!(failed.is_empty(), "{failed:?}");
    let obs = |label: &str| {
        t["observations"].as_array().unwrap().iter().find(|o| o["label"] == label).cloned().unwrap_or_else(|| panic!("no observation {label:?}"))
    };
    // The couch trigger's script started the conversation (StartConversation), and the
    // opening line's begin script ran.
    assert_eq!(obs("psych test opened by the couch trigger")["choices"], 6);
    assert_eq!(obs("psych test begun")["var.bBeganTest"], 1.0);
    assert_eq!(obs("psych test over")["state"], "ended");
    let tags = obs("tag skills after the psych test");
    assert_eq!((&tags["tag.Guns"], &tags["tag.Science"], &tags["tag.Sneak"], &tags["tag.Speech"]), (&Json::Bool(true), &Json::Bool(true), &Json::Bool(true), &Json::Bool(false)));
    assert_eq!(obs("Doc's farewell greeting")["info"], "0x001057E8");
    assert_eq!(obs("farewell over")["state"], "ended");
    let end = obs("VCG01 at the end");
    assert_eq!((&end["stage"], &end["completed"], &end["running"]), (&Json::from(200), &Json::Bool(true), &Json::Bool(false)));
    assert_eq!(obs("VCG02 after VCG01")["stage"], 5);
    // The two conversations: 15 and 5 lines, both ended by a Goodbye line.
    let ended: Vec<(&str, u64)> = t["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "dialogue_ended")
        .map(|e| (e["reason"].as_str().unwrap(), e["lines"].as_u64().unwrap()))
        .collect();
    assert_eq!(ended, vec![("goodbye", 15), ("goodbye", 5)]);
    // The idles asked of Doc Mitchell at the bedside (docs/spec-notes/idles.md §2): the
    // gestures named on his lines (VCG01DocWhoaThere, SitChairTalkPlayerD and A), the
    // script's `PlayIdle VCG01DocGiveMirror` at stage 30, and the same idle on the next
    // line. (The player's package idles need packages run, which the walk-through's
    // runner does not do.)
    let asked: Vec<(&str, &str, &str)> = t["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "idle_asked")
        .map(|e| (e["actor"].as_str().unwrap(), e["idle"].as_str().unwrap(), e["source"].as_str().unwrap()))
        .collect();
    assert!(asked.iter().all(|(actor, ..)| *actor == "0x00104C0F"), "{asked:?}");
    let bedside: Vec<(&str, &str)> = asked.iter().take(5).map(|(_, idle, source)| (*idle, *source)).collect();
    assert_eq!(
        bedside,
        vec![("0x0012280E", "speaker"), ("0x00024F9B", "speaker"), ("0x00024F98", "speaker"), ("0x0010658C", "script"), ("0x0010658C", "speaker")]
    );
}
