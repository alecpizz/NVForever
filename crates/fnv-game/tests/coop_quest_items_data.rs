//! What the host's quests hand a co-op guest, over the masters' own quest records (`fnv_game::coop_rewards`): the host runs each quest of a matrix
//! (every stage's result scripts in order, then the result scripts of the quest's
//! dialogue lines), and two guests take what it handed out:
//! - guest A, a fresh game, gets every item: a gun with its rounds, quest items, keys and
//!   notes (a Caravan deck once);
//! - guest B, a game that played those quests itself (the same stages set in its own
//!   game, as console `SetStage` calls would), gets none of them again and nothing it
//!   holds stacks up.
//!
//! The quests are found by their names in the data (FULL), so the matrix is the masters'
//! own records, not FormIDs typed here: Doc Mitchell's and Sunny Smiles' quests, Ghost
//! Town Gunfight, the Platinum Chip, the Strip, each faction's ending, and each add-on's
//! first quests.
//!
//! Needs the game data, which is never in the repository: set `FNVLAB_DATA_DIR` to the
//! game's `Data` folder with the base game and the add-ons. Without it the test prints
//! why it was skipped and passes.

use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::coop::CheckpointGear;
use fnv_game::coop_rewards::Gift;
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::forms::ItemKind;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;
use std::collections::BTreeMap;
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

/// The matrix, by quest name as the data spells it.
const QUESTS: [&str; 18] = [
    // Doc Mitchell: the Pip-Boy, the Vault 21 jumpsuit and his gifts.
    "Ain't That a Kick in the Head",
    // Sunny Smiles: the varmint rifle and its rounds.
    "Back in the Saddle",
    "Ghost Town Gunfight",
    // Benny and the Platinum Chip.
    "They Went That-a-Way",
    "Ring-a-Ding-Ding!",
    "The House Always Wins, I",
    // Each faction's ending.
    "All or Nothing",
    "Eureka!",
    "Veni, Vidi, Vici",
    "No Gods, No Masters",
    // The add-ons' starts. Dead Money's collar, suit, holorifle and rounds are its
    // master quest's stage 5 (DeadMoney.esm QUST 0x01000FBF) [observed: the data].
    "Sierra Madre Grand Opening!",
    "MQ01 Master Quest",
    "Fires in the Sky",
    "Happy Trails Expedition",
    "Midnight Science-Fiction Feature!",
    // Old World Blues' lab coat: stage 10 (OldWorldBlues.esm QUST 0x01002FCB).
    "Welcome to the Big Empty",
    "The Job",
    "The Reunion",
];

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

fn game(l: &Loaded) -> Game<'_> {
    let world = World::new(&l.forms, &l.refs, SimConfig::default());
    let mut g = Game::new(&l.data, world, GameConfig::default());
    g.new_game();
    g
}

/// The quests of the matrix found in the data, by name (a name two quests share: each).
fn matrix(data: &GameData) -> Vec<(String, FormId)> {
    let mut out = Vec::new();
    for name in QUESTS {
        let found: Vec<FormId> = data.quests.iter().filter(|(_, q)| q.name.as_deref() == Some(name)).map(|(id, _)| *id).collect();
        if found.is_empty() {
            eprintln!("note: no quest named {name:?} in the data");
        }
        out.extend(found.into_iter().map(|q| (name.to_string(), q)));
    }
    out
}

/// A dialogue line: its FormID, fixed speaker and begin and end result scripts.
type Line = (FormId, Option<FormId>, Option<usize>, Option<usize>);

/// Play `quest` in `g`: each stage in order (its result scripts, as SetStage runs them),
/// then each of its dialogue lines' result scripts (begin, end), spoken by the line's
/// speaker when it names one.
fn play(g: &mut Game, quest: FormId) {
    let stages: Vec<i32> = g.data.quests[&quest].stages.keys().copied().collect();
    for s in stages {
        g.set_stage(quest, s);
    }
    let mut lines: Vec<Line> = g.data.infos.iter().filter(|(_, i)| i.quest == Some(quest)).map(|(id, i)| (*id, i.speaker, i.begin, i.end)).collect();
    lines.sort_by_key(|l| l.0);
    for (_, speaker, begin, end) in lines {
        for u in [begin, end].into_iter().flatten() {
            g.run_result(u, RunCtx { self_ref: speaker, action_ref: None, quest: Some(quest) });
        }
    }
}

fn kind(g: &Game, item: FormId) -> Option<ItemKind> {
    g.world.forms.item(item).map(|i| i.kind)
}

fn held(g: &mut Game, item: FormId) -> i32 {
    g.world.item_count(FormId::PLAYER_REF, item).unwrap_or(0)
}

#[test]
fn the_hosts_quest_items_reach_a_fresh_guest_and_not_one_that_played_them() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let quests = matrix(&l.data);
    assert!(quests.len() >= 12, "most of the matrix is in the data: {quests:?}");

    let mut host = game(&l);
    host.coop_rewards.on = true;
    let mut by_quest: BTreeMap<FormId, Vec<Gift>> = BTreeMap::new();
    for (_, q) in &quests {
        play(&mut host, *q);
        by_quest.entry(*q).or_default().extend(std::mem::take(&mut host.coop_rewards.gifts));
    }

    // Guest B played the same quests in its own game.
    let mut b = game(&l);
    for (_, q) in &quests {
        play(&mut b, *q);
    }

    let mut a = game(&l);
    let (mut a_last, mut b_last) = (None, None);
    let mut handed = 0;
    let mut gun_and_rounds = Vec::new();
    for (name, q) in &quests {
        let gifts = &by_quest[q];
        let list: Vec<String> = gifts.iter().map(|g| format!("{} x{}{}", l.data.name(g.item), g.count, g.stage.map_or(String::new(), |s| format!(" (stage {s})")))).collect();
        eprintln!("{name} {q}: {} hand-out(s): {}", gifts.len(), list.join(", "));
        for gift in gifts {
            assert_eq!(gift.quest, Some(*q), "{name}: the batch names its quest");
            handed += 1;
            // Guest A: fresh, takes everything (a Caravan card goes to its cards, a note
            // to its notes).
            let before = held(&mut a, gift.item);
            let checkpoint = a.coop_checkpoint_of(*q);
            let n = a.take_coop_gift(gift, &mut a_last).unwrap();
            let k = kind(&a, gift.item);
            let card = k == Some(ItemKind::CaravanCard) || l.data.caravan_cards.contains_key(&gift.item);
            if checkpoint == CheckpointGear::Out {
                // Dead Money's gear: guest A is not in the Sierra Madre.
                assert_eq!((n, held(&mut a, gift.item)), (0, before), "{name}: guest A outside gets no {}", l.data.name(gift.item));
            } else if card {
                assert!(n > 0 || !a.player.caravan.cards.is_empty(), "{name}: guest A gets the deck");
            } else if k == Some(ItemKind::Note) {
                assert!(a.player.notes.contains(&gift.item), "{name}: guest A has the note {}", l.data.name(gift.item));
            } else if matches!(k, Some(ItemKind::Ammo | ItemKind::Aid | ItemKind::CasinoChip | ItemKind::CaravanMoney)) || before == 0 {
                assert!(n > 0 && held(&mut a, gift.item) > before, "{name}: guest A gets {} ({:?})", l.data.name(gift.item), k);
            }
            // Guest B: did it in its own game; takes nothing, holds no more than before.
            let b_before = held(&mut b, gift.item);
            assert!(b.coop_gift_done_here(gift), "{name}: guest B's own game did {q}");
            assert_eq!(b.take_coop_gift(gift, &mut b_last).unwrap(), 0, "{name}: guest B keeps its own {}", l.data.name(gift.item));
            assert_eq!(held(&mut b, gift.item), b_before, "{name}: nothing stacks on guest B");
        }
        // A gun and its rounds in one batch: guest A has both.
        let mut batches: BTreeMap<u32, Vec<&Gift>> = BTreeMap::new();
        for g in gifts {
            batches.entry(g.batch).or_default().push(g);
        }
        for items in batches.values() {
            let gun = items.iter().find(|g| kind(&a, g.item) == Some(ItemKind::Weapon));
            let ammo = items.iter().find(|g| kind(&a, g.item) == Some(ItemKind::Ammo));
            if let (Some(gun), Some(ammo)) = (gun, ammo) {
                if a.coop_checkpoint_of(*q) == CheckpointGear::Out {
                    continue;
                }
                assert!(held(&mut a, gun.item) > 0 && held(&mut a, ammo.item) >= ammo.count, "{name}: guest A has {} and its rounds", l.data.name(gun.item));
                gun_and_rounds.push(format!("{} + {} x{}", l.data.name(gun.item), l.data.name(ammo.item), ammo.count));
            }
        }
    }
    eprintln!("{handed} hand-outs over {} quests; gun and rounds together: {gun_and_rounds:?}", quests.len());
    assert!(handed > 0, "the matrix hands out items");
    assert!(!gun_and_rounds.is_empty(), "Sunny's varmint rifle and its rounds come as one batch");
}

/// Quest items are hand-outs now [design] (they were the host's before): a
/// quest item a stage's script gives the host is in the batch, and a guest that already
/// holds one gets no second.
#[test]
fn a_quest_item_is_handed_out_once() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let mut host = game(&l);
    host.coop_rewards.on = true;
    let mut quest_items = Vec::new();
    for (_, q) in matrix(&l.data) {
        play(&mut host, q);
        quest_items.extend(std::mem::take(&mut host.coop_rewards.gifts).into_iter().filter(|g| host.world.is_quest_item(g.item) && kind(&host, g.item) != Some(ItemKind::Note)));
    }
    eprintln!("quest items handed out: {:?}", quest_items.iter().map(|g| l.data.name(g.item)).collect::<Vec<_>>());
    assert!(!quest_items.is_empty(), "the matrix hands out quest items");
    let mut a = game(&l);
    let mut last = None;
    for g in &quest_items {
        let fresh = Gift { stage: None, quest: None, ..*g };
        a.take_coop_gift(&fresh, &mut last).unwrap();
        let once = held(&mut a, g.item);
        assert_eq!(a.take_coop_gift(&fresh, &mut last).unwrap(), 0, "a second {} is not given", l.data.name(g.item));
        assert_eq!(held(&mut a, g.item), once);
    }
}

/// Doc Mitchell's hand-out reaches a fresh guest whole: the 6 bobby pins (a misc stack,
/// once 1 on the guest [design: a found bug]) and every 9mm round his script adds one at a time.
#[test]
fn doc_mitchells_stacks_reach_a_guest_whole() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let Some(vcg01) = l.data.by_edid("VCG01") else { return };
    let mut host = game(&l);
    host.coop_rewards.on = true;
    play(&mut host, vcg01);
    let gifts = std::mem::take(&mut host.coop_rewards.gifts);
    let lockpick = l.data.by_edid("Lockpick").expect("the bobby pin's record");
    let ammo = l.data.by_edid("Ammo9mm").expect("the 9mm round's record");
    let total = |item: FormId| gifts.iter().filter(|g| g.item == item).map(|g| g.count).sum::<i32>();
    let (pins, rounds) = (total(lockpick), total(ammo));
    eprintln!("VCG01 hands out {pins} bobby pins and {rounds} 9mm rounds in {} gift(s)", gifts.iter().filter(|g| g.item == ammo).count());
    assert!(pins > 1 && rounds > 1, "Doc Mitchell hands out stacks: {pins} pins, {rounds} rounds");
    let mut a = game(&l);
    let (pins_before, rounds_before) = (held(&mut a, lockpick), held(&mut a, ammo));
    assert_eq!(pins_before, 0, "a new game has no bobby pins");
    let mut last = None;
    let mut taken = BTreeMap::new();
    for g in &gifts {
        let fresh = Gift { stage: None, quest: None, ..*g };
        *taken.entry(g.item).or_insert(0) += a.take_coop_gift(&fresh, &mut last).unwrap();
    }
    assert_eq!((taken[&lockpick], held(&mut a, lockpick)), (pins, pins), "the guest gets all {pins} bobby pins");
    assert_eq!((taken[&ammo], held(&mut a, ammo)), (rounds, rounds_before + rounds), "the guest gets all {rounds} rounds");
}

/// A guest that only half finished a quest in its own game (the stages before a stage
/// that hands something out set, the quest not completed) still gets what the host's
/// later stage hands out, and nothing of the stages it did (design).
#[test]
fn a_half_finished_quest_gives_the_later_stages_items() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let mut host = game(&l);
    host.coop_rewards.on = true;
    let mut checked = Vec::new();
    for (name, q) in matrix(&l.data) {
        play(&mut host, q);
        let gifts: Vec<Gift> = std::mem::take(&mut host.coop_rewards.gifts).into_iter().filter(|g| g.stage.is_some()).collect();
        let mut stages: Vec<i32> = gifts.iter().filter_map(|g| g.stage).collect();
        stages.sort();
        stages.dedup();
        if stages.is_empty() {
            continue;
        }
        // Guest C set the stages before the last stage that hands something out.
        let last = *stages.last().unwrap();
        let mut c = game(&l);
        let done: Vec<i32> = l.data.quests[&q].stages.keys().copied().filter(|s| *s < last).collect();
        for s in &done {
            c.set_stage(q, *s);
        }
        if done.is_empty() || c.quests.get(&q).is_some_and(|s| s.completed) {
            continue;
        }
        let mut lastb = None;
        for g in &gifts {
            let did = g.stage.is_some_and(|s| s < last);
            assert_eq!(c.coop_gift_done_here(g), did, "{name} stage {:?}", g.stage);
            let before = held(&mut c, g.item);
            let n = c.take_coop_gift(g, &mut lastb).unwrap();
            if did {
                assert_eq!(n, 0, "{name}: {} of a stage guest C did is not given again", l.data.name(g.item));
                assert_eq!(held(&mut c, g.item), before);
            } else if before == 0 && !matches!(kind(&c, g.item), Some(ItemKind::Note | ItemKind::CaravanCard)) {
                assert!(n > 0, "{name}: guest C gets {} of stage {last}, which it has not done", l.data.name(g.item));
            }
        }
        checked.push(format!("{name}: did {} stage(s) before {last}", done.len()));
    }
    eprintln!("half-finished quests checked: {checked:?}");
    assert!(!checked.is_empty(), "some quest of the matrix hands out items at a stage after others");
}

/// A guest that completed a quest in its own game still takes the host's completion XP
/// and caps for it (`coop_rewards::REPEAT_DONE_REWARDS` on) [design]: only
/// items keep the no-repeat rule.
#[test]
fn a_completed_quests_xp_and_caps_still_reach_the_guest() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let q = matrix(&l.data).into_iter().find(|(n, _)| n == "Back in the Saddle").expect("Back in the Saddle").1;
    let mut b = game(&l);
    play(&mut b, q);
    assert!(b.quests[&q].completed, "Back in the Saddle completes when all its stages are set");
    assert!(!b.coop_reward_done_here(q), "the reward is taken again");
    let caps = held(&mut b, fnv_game::coop_rewards::CAPS);
    let xp = b.take_coop_reward(100.0, 50).unwrap();
    assert!(xp > 0.0, "guest B gains the XP");
    assert_eq!(held(&mut b, fnv_game::coop_rewards::CAPS), caps + 50, "guest B gains the caps");
}

/// Items the add-ons' object scripts give the player (design: a fresh guest
/// gets every quest item, also those a quest's or a quest object's script gives).
/// Activated by the player on a fresh host, each row's script hands out for the quest it
/// names (Dead Money's Tampico prop, SCPT 0x0100FFD4: two items and NVDLC01MQ03b's
/// objective 15 [observed: the data]) or not at all (a pickup that names no quest).
#[test]
fn quest_objects_scripts_hand_out_for_their_quest() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    // (script, label) [observed: the add-ons' scripts that AddItem to the player].
    let rows: [(u32, &str); 5] = [
        (0x0100_FFD4, "Dead Money: Tampico props (names NVDLC01MQ03b)"),
        (0x0201_12F7, "Honest Hearts: OnActivate pickup"),
        (0x0301_2F01, "Old World Blues: OnActivate weapon pickup"),
        (0x0400_E106, "Lonesome Road: OnActivate pickup"),
        (0x0400_D792, "Lonesome Road: OnActivate pickup"),
    ];
    let mut out = Vec::new();
    let mut dm = None;
    for (script, label) in rows {
        let script = FormId(script);
        let Some(placed) = l.refs.refs.values().find(|r| l.data.script_of_base(r.base) == Some(script)).map(|r| r.id) else {
            out.push(format!("{label} {script}: no placed reference"));
            continue;
        };
        let mut host = game(&l);
        host.coop_rewards.on = true;
        let ctx = RunCtx { self_ref: Some(placed), action_ref: Some(FormId::PLAYER_REF), quest: None };
        host.run_event(fnv_game::game::InstKey::Ref { reference: placed }, script, fnv_game::game::event::ON_ACTIVATE, Some(fnv_script::vm::Value::Ref(FormId::PLAYER_REF.0)), ctx);
        let gifts = std::mem::take(&mut host.coop_rewards.gifts);
        let list: Vec<String> = gifts.iter().map(|g| format!("{} x{} for {}", l.data.name(g.item), g.count, g.quest.map_or("-".into(), |q| l.data.name(q)))).collect();
        out.push(format!("{label} {script} on {placed}: {} hand-out(s) {list:?}", gifts.len()));
        if script == FormId(0x0100_FFD4) {
            dm = Some(gifts);
        }
    }
    eprintln!("add-on object scripts:\n  {}", out.join("\n  "));
    let dm = dm.expect("Dead Money's Tampico prop is placed");
    assert!(!dm.is_empty() && dm.iter().all(|g| g.quest == Some(FormId(0x0101_3A43))), "the Tampico props hand out for NVDLC01MQ03b: {dm:?}");
    // Guest A takes them inside the Sierra Madre (its items in Dead Money's checkpoint,
    // [design]); guest B, whose game completed NVDLC01MQ03b, keeps its own.
    let (mut a, mut b) = (game(&l), game(&l));
    a.coop_check_in(DM_CHECKPOINT, -1, None);
    b.quest(FormId(0x0101_3A43)).completed = true;
    let (mut la, mut lb) = (None, None);
    for g in &dm {
        assert!(a.take_coop_gift(g, &mut la).unwrap() > 0, "guest A gets {}", l.data.name(g.item));
        assert_eq!(b.take_coop_gift(g, &mut lb).unwrap(), 0, "guest B keeps its own {}", l.data.name(g.item));
    }
}

/// Dead Money's checkpoint: the bunker's `player.RemoveAllItems` into
/// NVDLC01PlayerEquipmentContainerRef (SCPT 0x0100C59E) [observed: the data].
const DM_CHECKPOINT: FormId = FormId(0x0100_EFCC);

/// Dead Money's gear lives in the Sierra Madre only [design]: the
/// collar, jumpsuit, holorifle and cells NVDLC01MQ01 stage 5 gives the host reach a
/// guest only while Dead Money's checkpoint holds that guest's items, as hand-outs and as
/// story gear; on the way out the guest loses the collar with the host's and keeps the
/// rest, its own items back (the original); a guest outside (a host that set the stage
/// in Goodsprings, a guest that joins later) gets none.
#[test]
fn dead_moneys_gear_stays_in_the_sierra_madre() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let strip = fnv_game::coop::strip_containers_of(&l.data);
    eprintln!("whole-inventory checkpoints: {strip:?}");
    assert!(strip.contains(&DM_CHECKPOINT), "the bunker's container is found from the scripts");
    let mq01 = FormId(0x0100_0FBF);
    let mut host = game(&l);
    host.coop_rewards.on = true;
    host.set_stage(mq01, 5);
    let gifts = std::mem::take(&mut host.coop_rewards.gifts);
    let gear = host.story_gear();
    let names = |v: &[FormId]| v.iter().map(|i| l.data.name(*i)).collect::<Vec<_>>();
    eprintln!("stage 5 hands out {:?}; the host wears {:?}", names(&gifts.iter().map(|g| g.item).collect::<Vec<_>>()), names(&gear.iter().map(|g| g.0).collect::<Vec<_>>()));
    let collar = FormId(0x0100_6E8F);
    assert!(gifts.iter().any(|g| g.item == collar) && gear.iter().any(|g| g.0 == collar), "stage 5 gives and locks on the collar");

    // Outside the Sierra Madre (Goodsprings): nothing, as hand-out or story gear.
    let mut out = game(&l);
    let mut last = None;
    for g in &gifts {
        assert_eq!(out.take_coop_gift(g, &mut last).unwrap(), 0, "outside: no {}", l.data.name(g.item));
    }
    let (on, _) = out.follow_story_gear(&gear);
    assert!(on.iter().all(|i| !gear.iter().any(|g| g.0 == *i && i.0 >> 24 == 1)), "outside: no Dead Money gear worn: {:?}", names(&on));
    for g in &gifts {
        assert_eq!(held(&mut out, g.item), 0, "outside: {} not held", l.data.name(g.item));
    }

    // Through the bunker: the guest's own items go into the checkpoint, the gear comes.
    let mut inside = game(&l);
    let pistol = FormId(0x000E_3778);
    inside.world.add_item(FormId::PLAYER_REF, pistol, 1).unwrap();
    assert!(inside.coop_check_in(DM_CHECKPOINT, -1, None) > 0, "the guest hands over its own");
    assert_eq!(held(&mut inside, pistol), 0);
    // The story gear first, then the hand-outs, as in the game (observed: buckallobill, in game, FNV 1.4.0.525).
    inside.follow_story_gear(&gear);
    let mut last = None;
    for g in &gifts {
        assert!(inside.take_coop_gift(g, &mut last).unwrap() > 0 || held(&mut inside, g.item) > 0, "inside: {} comes", l.data.name(g.item));
    }
    assert!(held(&mut inside, collar) > 0, "inside: the collar");

    // The host's way out: NVDLC01MQ03 stage 100, then the return trigger (SCPT
    // 0x01013FB4 on REFR 0x01013FB3) gives the host's own items back: no hand-outs.
    host.world.add_item(DM_CHECKPOINT, pistol, 1).unwrap();
    host.set_stage(FormId(0x0100_0FC1), 100);
    std::mem::take(&mut host.coop_rewards.gifts);
    let ctx = RunCtx { self_ref: Some(FormId(0x0101_3FB3)), action_ref: Some(FormId::PLAYER_REF), quest: None };
    host.run_event(fnv_game::game::InstKey::Ref { reference: FormId(0x0101_3FB3) }, FormId(0x0101_3FB4), fnv_game::game::event::ON_TRIGGER, Some(fnv_script::vm::Value::Ref(FormId::PLAYER_REF.0)), ctx);
    assert!(held(&mut host, pistol) > 0, "the return trigger gives the host its items back");
    let back: Vec<String> = host.coop_rewards.gifts.iter().map(|g| l.data.name(g.item)).collect();
    assert!(back.is_empty(), "the host's own items given back are no hand-outs: {back:?}");

    // The way out as the original: NVDLC01MQ03 stage 100 takes the host's collar (its
    // story gear now lacks it), the guest loses it too; the checkpoint gives the guest's
    // own items back; the jumpsuit, holorifle and cells stay, the guest's own now.
    let gear_out = host.story_gear();
    assert!(!gear_out.iter().any(|g| g.0 == collar), "stage 100 takes the host's collar: {:?}", names(&gear_out.iter().map(|g| g.0).collect::<Vec<_>>()));
    inside.follow_story_gear(&gear_out);
    assert_eq!(held(&mut inside, collar), 0, "the collar goes with the host's");
    assert!(inside.coop_check_out(DM_CHECKPOINT) > 0);
    assert_eq!(held(&mut inside, pistol), 1, "the guest's own pistol is back");
    inside.follow_story_gear(&gear_out);
    let jumpsuit = FormId(0x0100_F3A4);
    for g in &gifts {
        let k = kind(&inside, g.item);
        if g.item != collar && k != Some(ItemKind::Note) {
            assert!(held(&mut inside, g.item) > 0, "out: the guest keeps {}", l.data.name(g.item));
        }
    }
    let worn = |g: &Game, i: FormId| g.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).is_some_and(|inv| inv.is_equipped(i));
    assert!(worn(&inside, jumpsuit), "the jumpsuit stays on, as the host's");
    // A guest that wore its own collar into the checkpoint (a quest item RemoveAllItems
    // leaves it, [documented: GECK wiki RemoveAllItems]) loses it with the host's too.
    let mut own = game(&l);
    own.world.add_item(FormId::PLAYER_REF, collar, 1).unwrap();
    own.world.equip(FormId::PLAYER_REF, collar).unwrap();
    own.coop_check_in(DM_CHECKPOINT, -1, None);
    own.follow_story_gear(&gear);
    assert_eq!(held(&mut own, collar), 1, "its own collar, kept on");
    own.follow_story_gear(&gear_out);
    assert_eq!(held(&mut own, collar), 0, "its own collar goes with the host's");
    // A guest that joins after the way out: none of it.
    let mut late = game(&l);
    let mut last = None;
    for g in &gifts {
        assert_eq!(late.take_coop_gift(g, &mut last).unwrap(), 0, "a late guest gets no {}", l.data.name(g.item));
    }

    // A guest that saves inside the Sierra Madre and loads (design): its save
    // is its own game with the character as it is now (`with_character_of`), the
    // checkpoint's records with it; after the load the way out still gives its own back.
    let mut saver = game(&l);
    saver.world.add_item(FormId::PLAYER_REF, pistol, 1).unwrap();
    saver.coop_check_in(DM_CHECKPOINT, -1, None);
    saver.follow_story_gear(&gear);
    let mut last = None;
    for g in &gifts {
        saver.take_coop_gift(g, &mut last).unwrap();
    }
    let base = game(&l).snapshot();
    let save = base.with_character_of(&saver.snapshot());
    let text = serde_json::to_string(&save).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
    assert!(back.coop_checked.contains_key(&DM_CHECKPOINT), "the save keeps what the checkpoint holds");
    assert!(!back.coop_checkpoint_gear.is_empty(), "the save keeps the checkpoint's gear handed out");
    let world = World::restore(&l.forms, &l.refs, &back.sim).unwrap();
    let mut loaded = Game::restore(&l.data, world, saver.cfg, &back).unwrap();
    assert_eq!(held(&mut loaded, pistol), 0, "loaded: the pistol is still in the checkpoint");
    assert!(held(&mut loaded, collar) > 0, "loaded: the collar is still on");
    loaded.follow_story_gear(&gear);
    loaded.follow_story_gear(&gear_out);
    assert_eq!(held(&mut loaded, collar), 0, "loaded: the collar goes with the host's");
    assert!(loaded.coop_check_out(DM_CHECKPOINT) > 0, "loaded: the checkpoint gives its items back");
    assert_eq!(held(&mut loaded, pistol), 1, "loaded: the guest's own pistol is back");
    assert!(held(&mut loaded, jumpsuit) > 0, "loaded: the jumpsuit is the guest's now");
}

/// A quest the host fails (a Fail Quest stage) is kept for its guests, as a completion
/// is (the host showed 10 "Quest failed" [observed: buckallobill, in game, FNV 1.4.0.525]; the guest none [design: a found bug]). The
/// guest lists it once as failed, and a completed quest that fails after is told again.
#[test]
fn the_hosts_failed_quest_is_kept_for_the_guests() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    // Every named quest with a Fail Quest stage, from the data (VMS16's stage 110 among
    // them [observed: `L5.quest.stage_order`]).
    let failing: Vec<(FormId, i32)> = l
        .data
        .quests
        .iter()
        .filter(|(_, q)| q.name.as_deref().is_some_and(|n| !n.trim().is_empty()))
        .filter_map(|(id, q)| q.stages.iter().find(|(_, es)| es.iter().any(|e| e.flags & fnv_game::quest::STAGE_FAIL != 0)).map(|(s, _)| (*id, *s)))
        .collect();
    assert!(failing.len() >= 5, "named quests with a Fail Quest stage: {}", failing.len());
    let (q, stage) = failing[0];
    let mut host = game(&l);
    host.coop_rewards.on = true;
    host.set_stage(q, stage);
    assert!(host.quests[&q].failed, "the stage fails {q}");
    assert_eq!(host.coop_rewards.ended, vec![(q, true)], "the host keeps {q} for its guests");
    // Not twice: a second fail stage of the same quest is no news.
    host.coop_rewards.ended.clear();
    host.set_stage(q, stage);
    assert!(host.coop_rewards.ended.is_empty());
    // A solo game keeps nothing.
    let mut solo = game(&l);
    solo.set_stage(q, stage);
    assert!(solo.quests[&q].failed && solo.coop_rewards.ended.is_empty());

    let mut guest = game(&l);
    guest.coop_guest = true;
    guest.coop_set_host_quest(Some(fnv_game::coop::HostQuest { quest: q, objectives: BTreeMap::from([(10, true), (20, false)]), completed: false, failed: false }));
    guest.coop_set_host_quest(None);
    assert_eq!(guest.coop_host_objectives.get(&q), Some(&BTreeMap::from([(10, true), (20, false)])), "the objectives the host's view showed stay for the ended quest's page");
    assert!(guest.coop_host_quest_ended(q, true), "news the first time");
    assert!(!guest.coop_host_quest_ended(q, true), "not told twice");
    assert!(!guest.coop_host_quest_ended(q, false), "a completion after a failure is no news");
    assert_eq!(guest.coop_host_ended, vec![(q, true)]);
    let other = failing[1].0;
    assert!(guest.coop_host_quest_ended(other, false), "completed");
    assert!(guest.coop_host_quest_ended(other, true), "then failed: told");
    assert_eq!(guest.coop_host_ended, vec![(q, true), (other, true)]);
    guest.coop_forget_host_quests();
    assert!(guest.coop_host_ended.is_empty() && guest.coop_host_objectives.is_empty());
}

/// A quest completed and failed in the same frame (in one sync tick) reaches the guests
/// as both, in that order, though its completion pays nothing (I FORGOT TO REMEMBER TO FORGET, VMS39, completes at
/// stage 70 and fails at stage 80 a second later [observed: buckallobill, in game, FNV 1.4.0.525];
/// the guest showed neither line [design: a found bug]).
#[test]
fn a_quest_completed_then_failed_in_one_tick_reaches_the_guest_as_both() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let q = l.data.by_edid("VMS39").expect("VMS39");
    let stages = &l.data.quests[&q].stages;
    let flagged = |f: u8| stages.iter().find(|(_, es)| es.iter().any(|e| e.flags & f != 0)).map(|(s, _)| *s);
    let complete = flagged(fnv_game::quest::STAGE_COMPLETE).expect("a Complete Quest stage");
    let fail = flagged(fnv_game::quest::STAGE_FAIL).expect("a Fail Quest stage");
    assert!(complete < fail, "VMS39 completes at {complete} before it fails at {fail}");
    let mut host = game(&l);
    host.coop_rewards.on = true;
    // Its first stage shows an objective (Travel to Bitter Springs with Boone
    // [observed: buckallobill, in game, FNV 1.4.0.525]).
    host.set_stage(q, *stages.keys().next().expect("a stage"));
    // Both stages in one frame: the host's sync takes them together.
    host.set_stage(q, complete);
    host.set_stage(q, fail);
    assert_eq!(host.coop_rewards.ended, vec![(q, false), (q, true)], "both, completed first");
    let mut guest = game(&l);
    guest.coop_guest = true;
    let told: Vec<bool> = std::mem::take(&mut host.coop_rewards.ended).into_iter().map(|(q, failed)| guest.coop_host_quest_ended(q, failed)).collect();
    assert_eq!(told, [true, true], "the guest tells both");
    assert_eq!(guest.coop_host_ended, vec![(q, true)], "and lists it as failed");
    // The host's story, which every guest's quest list reads (a guest that joins later
    // too), carries it as failed with its objectives (design: the guest listed
    // only the host's active quest).
    let view = host.story_view();
    let s = &view.quests[&q];
    assert!(s.ended && s.failed, "ended and failed in the story");
    assert!(!s.objectives.is_empty(), "with the objectives the host was shown");
    let shown: Vec<i32> = host.quests[&q].objectives.iter().filter(|(_, o)| o.displayed || o.completed).map(|(i, _)| *i).collect();
    assert_eq!(s.objectives.keys().copied().collect::<Vec<_>>(), shown);
}
