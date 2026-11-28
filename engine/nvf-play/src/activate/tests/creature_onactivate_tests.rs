//! #3669: E on a living non-hostile creature shown by name only (`Action::Name`) whose
//! script has an `OnActivate` block runs that block (e.g. `CRBrahminScript` brahmin
//! tipping) while keeping the name-only prompt and "nothing" default activation behind
//! it. Dead creatures are still searched, creatures without an `OnActivate` block are
//! unchanged, and hostile actors keep #2536's rule.

use super::*;
use crate::script::{with_world, ScriptState};
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_game::functions::senses::Motion;
use fnv_game::game::event;
use fnv_game::trace::EventKind;

const BRAHMIN_SCRIPT: u32 = 0x910;
const PUSH_IDLE: u32 = 0x911;
const GREETING_TOPIC: u32 = 0x912;
const GREETING_INFO: u32 = 0x913;
const BRAHMIN_COUNT: u32 = 0x914;

fn sub(sig: &[u8; 4], data: impl Into<Vec<u8>>) -> Subrecord {
    Subrecord { sig: *sig, data: data.into(), extended: false }
}

fn record(sig: &[u8; 4], id: u32, flags: u32, subs: &[Subrecord]) -> Vec<u8> {
    let body = write_subrecords(subs);
    [&sig[..], &(body.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &id.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &body].concat()
}

fn grup(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
}

/// Minimal compiled stub header with enabled flag (`flags = 1`) paired with `SCTX`
/// declaring a `string_var` so `Auto` source routing compiles the script from source.
fn source_script(id: u32, edid: &str, body: &str) -> Vec<u8> {
    // Minimal valid SCDA (scn header + empty block): 0x001D, len 0.
    let scda: Vec<u8> = [&0x001Du16.to_le_bytes()[..], &0u16.to_le_bytes()].concat();
    let schr = [&[0u8; 8][..], &(scda.len() as u32).to_le_bytes(), &[0u8; 4], &0u16.to_le_bytes(), &1u16.to_le_bytes()].concat();
    let source = format!("scn {edid}\n{body}");
    record(
        b"SCPT",
        id,
        0,
        &[
            sub(b"EDID", format!("{edid}\0").into_bytes()),
            sub(b"SCHR", schr),
            sub(b"SCDA", scda),
            sub(b"SCTX", source.into_bytes()),
        ],
    )
}

/// Build a synthetic plugin with `ROBOT` (reusing the test world's placed creature
/// `ROBOT_REF`) optionally carrying a `CRBrahminScript`-style `OnActivate` + `GameMode`
/// script, a push `IDLE` form, and a generic `GREETING` line (to prove that calling plain
/// `Activate` on a name-only creature never opens a conversation).
fn plugin(with_on_activate: bool) -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0u32.to_le_bytes(), 0x2000u32.to_le_bytes()].concat();
    let mut out = record(b"TES4", 0, 1, &[sub(b"HEDR", hedr)]);

    // Global counter incremented by OnActivate.
    let glob = record(b"GLOB", BRAHMIN_COUNT, 0, &[sub(b"EDID", *b"TestBrahminCount\0"), sub(b"FNAM", *b"s"), sub(b"FLTV", 0.0f32.to_le_bytes())]);
    out.extend(grup(*b"GLOB", 0, &glob));

    // Push idle form (`Loose1stPBrahminPush`-equivalent) with a `.kf` MODL path.
    let idle = record(b"IDLE", PUSH_IDLE, 0, &[sub(b"EDID", *b"TestBrahminPushIdle\0"), sub(b"MODL", *b"characters\\_male\\idleanims\\brahminpush.kf\0")]);
    out.extend(grup(*b"IDLE", 0, &idle));

    // Script modeled on CRBrahminScript (#3669):
    // - OnActivate by player on a living brahmin while sneaking with weapon away:
    //   arms a 0.8 s timer.
    // - Otherwise (standing, weapon out, or dead): calls plain `Activate`.
    // - GameMode plays the push idle and at 0.8 s calls `player.PushActorAway me 1`.
    let script_body = "\
string_var sSrc
short iState
float fTimer
ref rSelf
Begin OnActivate
  set TestBrahminCount to TestBrahminCount + 1
  if GetDead == 0 && IsActionRef player == 1 && player.IsSneaking == 1 && player.IsWeaponOut == 0
    if iState == 0
      set iState to 1
      set fTimer to 0.8
    endif
  else
    Activate
  endif
End
Begin GameMode
  if iState == 1
    player.PlayIdle TestBrahminPushIdle
    set iState to 2
  elseif iState == 2
    set fTimer to fTimer - GetSecondsPassed
    if fTimer <= 0
      set rSelf to GetSelf
      player.PushActorAway rSelf 1
      set iState to 0
    endif
  endif
End
";
    if with_on_activate {
        out.extend(grup(*b"SCPT", 0, &source_script(BRAHMIN_SCRIPT, "TestBrahminScript", script_body)));
    }

    // Creature record without Allow PC Dialogue (ACBS flags = 0).
    let mut crea_subs = vec![sub(b"EDID", *b"TestBrahmin\0"), sub(b"FULL", *b"Brahmin\0"), sub(b"ACBS", [0u8; 24])];
    if with_on_activate {
        crea_subs.push(sub(b"SCRI", BRAHMIN_SCRIPT.to_le_bytes()));
    }
    out.extend(grup(*b"CREA", 0, &record(b"CREA", ROBOT, 0, &crea_subs)));

    // Interior cell CELL_EXT + persistent ACRE placement for ROBOT_REF so SimState and
    // Game share the exact plugin FormStore, RefTable, SCRI, and GLOB.
    let cell_rec = record(b"CELL", CELL_EXT, 0, &[sub(b"EDID", *b"BrahminCell\0"), sub(b"DATA", [1u8])]);
    let mut pos = Vec::with_capacity(24);
    for v in [60.0f32, 0.0, 0.0, 0.0, 0.0, 0.0] {
        pos.extend(v.to_le_bytes());
    }
    let acre = record(b"ACRE", ROBOT_REF, 0x0400, &[sub(b"NAME", ROBOT.to_le_bytes()), sub(b"DATA", pos)]);
    let cell_children = grup(CELL_EXT.to_le_bytes(), 6, &grup(CELL_EXT.to_le_bytes(), 8, &acre));
    let cell_block = grup(0u32.to_le_bytes(), 2, &grup(0u32.to_le_bytes(), 3, &[cell_rec, cell_children].concat()));
    out.extend(grup(*b"CELL", 0, &cell_block));

    // Generic GREETING topic + line (unconditioned) so if default `Activate` erroneously
    // fell through to `start_dialogue`, a conversation would open.
    let dial = record(b"DIAL", GREETING_TOPIC, 0, &[sub(b"EDID", *b"GREETING\0"), sub(b"DATA", [0u8, 0])]);
    let mut trdt = vec![0u8; 24];
    trdt[12] = 1;
    let info = record(b"INFO", GREETING_INFO, 0, &[sub(b"DATA", [0u8, 0, 0]), sub(b"TRDT", trdt), sub(b"NAM1", *b"Moo!\0")]);
    out.extend(grup(*b"DIAL", 0, &[dial, grup(GREETING_TOPIC.to_le_bytes(), 7, &info)].concat()));

    out
}

fn brahmin_scene(with_on_activate: bool, hostile: bool) -> App {
    let bytes = plugin(with_on_activate);
    let mut script = ScriptState::over_plugins(&[fnv_esm::Plugin::parse(&bytes, "BrahminTest.esm".into()).unwrap()]);
    let (forms, refs) = {
        let g = &mut script.rt.as_mut().unwrap().game;
        if with_on_activate {
            let unit = g.data.script(FormId(BRAHMIN_SCRIPT)).expect("TestBrahminScript must be compiled and admitted");
            assert!(unit.has_block(event::ON_ACTIVATE), "TestBrahminScript must carry an OnActivate block");
            assert_eq!(g.ref_script(FormId(ROBOT_REF)), Some(FormId(BRAHMIN_SCRIPT)), "brahmin ref must resolve SCRI");
        }
        if hostile {
            g.awareness.forced.insert((FormId(ROBOT_REF), FormId::PLAYER_REF));
        }
        (g.world.forms, g.world.placed)
    };
    let mut app = scene(&[(ROBOT_REF, 60.0)]);
    app.insert_resource(SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) });
    app.insert_resource(script);
    // Ensure the player and brahmin share the loaded cell so GameMode runs on the creature.
    runtime(&mut app, |rt| {
        rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL_EXT));
        rt.game.player_entered(Some(FormId(CELL_EXT)));
    });
    app
}

fn runtime<R>(app: &mut App, f: impl FnOnce(&mut crate::script::Runtime) -> R) -> R {
    app.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, f)
    })
}

fn set_player_state(app: &mut App, sneaking: bool, weapon_out: bool) {
    runtime(app, |rt| {
        rt.game.set_motion(FormId::PLAYER_REF, Motion { sneaking, ..Default::default() });
        rt.game.set_weapon_out(FormId::PLAYER_REF, weapon_out);
    });
}

fn script_activated_count(app: &mut App) -> f32 {
    runtime(app, |rt| rt.game.world.global(FormId(BRAHMIN_COUNT)).unwrap_or(0.0))
}

fn unskipped_talk_events(app: &mut App) -> usize {
    runtime(app, |rt| {
        rt.game
            .events
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                !rt.is_event_skipped(*i)
                    && matches!(&e.kind, EventKind::Activated { outcome: Some(o), .. } if crate::script_events::activation_from_json(o).is_some_and(|a| matches!(a.outcome, fnv_sim::world::Outcome::Talk { .. })))
            })
            .count()
    })
}

#[test]
fn sneaking_weapon_away_pushes_brahmin_down_and_it_gets_up() {
    let mut app = brahmin_scene(true, false);
    set_player_state(&mut app, true, false);
    app.update();

    // Prompt stays name-only (`Action::Name`) while living and non-hostile.
    let shown = app.world().resource::<Target>().0.as_ref().map(|t| (t.form_id, t.action.clone(), t.prompted()));
    assert_eq!(shown, Some((ROBOT_REF, Action::Name, true)));

    // Pressing E runs OnActivate and arms the push sequence (no conversation, no HUD error).
    press_e(&mut app);
    assert_eq!(script_activated_count(&mut app), 1.0, "OnActivate must run on E");
    assert!(runtime(&mut app, |rt| rt.game.dialogue.is_none()), "must not open dialogue");
    assert!(app.world().resource::<HudMessage>().text().is_empty(), "must not show 'nothing to say'");

    // Run GameMode ticks: first plays the push idle, then 0.8 s later PushActorAway knocks it down.
    let mut saw_idle = false;
    let mut knocked_down = false;
    for _ in 0..80 {
        runtime(&mut app, |rt| {
            rt.game.tick();
            saw_idle |= rt.game.events.iter().any(|e| matches!(e.kind, EventKind::IdleAsked { actor, idle, .. } if actor == FormId::PLAYER_REF && idle == FormId(PUSH_IDLE)));
            knocked_down |= rt.game.knockout(FormId(ROBOT_REF)).is_some_and(|k| k.pushed);
        });
        if knocked_down {
            break;
        }
    }
    assert!(saw_idle, "GameMode must play the brahmin push idle");
    assert!(knocked_down, "PushActorAway must knock the brahmin down after 0.8 s");

    // Advance until the brahmin gets back up.
    let mut stood_up = false;
    for _ in 0..600 {
        runtime(&mut app, |rt| {
            rt.game.tick();
            stood_up = rt.game.knockout(FormId(ROBOT_REF)).is_none();
        });
        if stood_up {
            break;
        }
    }
    assert!(stood_up, "knocked-down brahmin must get up again");
}

#[test]
fn standing_or_weapon_out_does_nothing_and_dead_brahmin_is_searched() {
    let mut app = brahmin_scene(true, false);

    // 1. Standing with weapon away: E runs OnActivate which calls plain `Activate`,
    //    but default activation stays "nothing" (no conversation, no unskipped Talk
    //    event that would emit "has nothing to say", no knockdown).
    set_player_state(&mut app, false, false);
    app.update();
    press_e(&mut app);
    assert_eq!(script_activated_count(&mut app), 1.0);
    assert!(runtime(&mut app, |rt| rt.game.dialogue.is_none()), "standing Activate must not open conversation");
    assert_eq!(unskipped_talk_events(&mut app), 0, "default Talk event must be skipped so no 'nothing to say' message fires");
    assert!(app.world().resource::<HudMessage>().text().is_empty());
    assert!(runtime(&mut app, |rt| rt.game.knockout(FormId(ROBOT_REF)).is_none()));

    // 2. Sneaking with weapon out: same — does nothing.
    set_player_state(&mut app, true, true);
    app.update();
    press_e(&mut app);
    assert_eq!(script_activated_count(&mut app), 2.0);
    assert!(runtime(&mut app, |rt| rt.game.dialogue.is_none()), "weapon-out Activate must not open conversation");
    assert_eq!(unskipped_talk_events(&mut app), 0, "weapon-out Talk event must be skipped");
    assert!(app.world().resource::<HudMessage>().text().is_empty());
    assert!(runtime(&mut app, |rt| rt.game.knockout(FormId(ROBOT_REF)).is_none()));

    // 3. Dead brahmin: crosshair becomes Action::Search and E opens its loot container.
    runtime(&mut app, |rt| {
        rt.game.extra.entry(FormId(ROBOT_REF)).or_default().dead = true;
    });
    app.update();
    let shown = app.world().resource::<Target>().0.as_ref().map(|t| (t.form_id, t.action.clone()));
    assert_eq!(shown, Some((ROBOT_REF, Action::Search)));
    press_e(&mut app);
    let opened = runtime(&mut app, |rt| {
        rt.game.events.iter().any(|e| {
            matches!(&e.kind, EventKind::Activated { reference, outcome: Some(o), .. }
                if *reference == FormId(ROBOT_REF)
                && crate::script_events::activation_from_json(o).is_some_and(|a| matches!(a.outcome, fnv_sim::world::Outcome::OpenedContainer { .. })))
        })
    });
    assert!(opened, "dead brahmin must still open its loot inventory on E");
}

#[test]
fn creature_without_onactivate_and_hostile_creature_are_unchanged() {
    // 1. Living creature without an OnActivate block: E does nothing, 0 activations.
    let mut plain = brahmin_scene(false, false);
    set_player_state(&mut plain, true, false);
    plain.update();
    assert_eq!(plain.world().resource::<Target>().0.as_ref().map(|t| t.action.clone()), Some(Action::Name));
    press_e(&mut plain);
    let plain_acts = runtime(&mut plain, |rt| rt.game.events.iter().filter(|e| matches!(e.kind, EventKind::Activated { .. })).count());
    assert_eq!(plain_acts, 0, "creature without OnActivate must not activate");

    // 2. Hostile creature WITH an OnActivate block keeps #2536's rule: E does not run OnActivate.
    let mut hostile = brahmin_scene(true, true);
    set_player_state(&mut hostile, true, false);
    hostile.update();
    assert_eq!(hostile.world().resource::<Target>().0.as_ref().map(|t| t.action.clone()), Some(Action::Hostile));
    press_e(&mut hostile);
    assert_eq!(script_activated_count(&mut hostile), 0.0, "hostile creature must keep #2536 rule");
}
