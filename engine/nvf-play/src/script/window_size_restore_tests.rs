//! Scoped regression test verifying that `Runtime::restore_save` preserves the live
//! window dimensions (`jip9.window`) across save loads so load-time scripts querying
//! display size or cursor coordinates read the current surface immediately.

use super::*;

#[test]
fn restore_save_preserves_live_window_dimensions() {
    let mut rt = ScriptState::over_no_plugins().rt.unwrap();
    let refs: &'static RefTable = Box::leak(Box::new(RefTable::default()));
    let forms = rt.game.world.forms;
    let mut sim = SimState {
        forms,
        refs,
        world: World::new(forms, refs, fnv_sim::SimConfig::default()),
    };
    let snap = with_world(&mut rt, &mut sim, |rt| rt.game.snapshot());

    // Simulate a live maximized ultrawide surface set before loading a save.
    rt.game.other.jip9.window = Some((3440.0, 1369.0));

    rt.restore_save(&mut sim, snap).unwrap();
    assert_eq!(rt.game.other.jip9.window, Some((3440.0, 1369.0)));
}
