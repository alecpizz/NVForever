//! Pending touch-only ownership retirement when an optional presentation changes.
//! The Android wrapper's sole reader publishes these into its neutral-recovery policy.
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Feature { Terminal, Companion, Lockpick, Dialogue }

#[derive(Resource, Default)]
pub struct Retirements {
    changed: u8,
    // Leaf barrier lasts through this frame; Root pending records outlive it.
    barrier: u8,
    contacts: [Option<(Entity, u64)>; 32],
    overflow: bool,
}
impl Retirements {
    pub fn changed(&self, feature: Feature) -> bool {
        self.changed & bit(feature) != 0
    }
    pub fn barrier(&self, feature: Feature) -> bool {
        self.barrier & bit(feature) != 0
    }
    pub fn contacts(&self) -> impl Iterator<Item = (Entity, u64)> + '_ {
        self.contacts.iter().flatten().copied()
    }
    pub fn overflowed(&self) -> bool { self.overflow }
    /// Root must retain returned contacts until actual End/Cancel/neutral, not one frame.
    pub fn take(&mut self) -> Self { std::mem::take(self) }
}

pub(crate) fn install(app: &mut App) {
    if !app.is_plugin_added::<RetirementPlugin>() { app.add_plugins(RetirementPlugin); }
}
struct RetirementPlugin;
impl Plugin for RetirementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Retirements>().add_systems(Last, end_frame);
    }
}
fn end_frame(mut pending: ResMut<Retirements>) {
    // Do not clear changed/contacts: Android may adopt a late callback next PreUpdate.
    if pending.barrier != 0 { pending.barrier = 0; }
}
fn bit(feature: Feature) -> u8 {
    match feature { Feature::Terminal => 1, Feature::Companion => 2, Feature::Lockpick => 4, Feature::Dialogue => 8 }
}

pub(crate) fn retire(world: &mut World, feature: Feature, window: Option<Entity>, ids: &[u64]) {
    let mut pending = world.resource_mut::<Retirements>();
    pending.changed |= bit(feature);
    pending.barrier |= bit(feature);
    if let Some(window) = window {
        for &id in ids {
            let contact = (window, id);
            if pending.contacts.contains(&Some(contact)) { continue; }
            if let Some(slot) = pending.contacts.iter_mut().find(|v| v.is_none()) { *slot = Some(contact); }
            else { pending.overflow = true; }
        }
    }
    // Only the sole wrapper publisher may convert these window-scoped records to
    // UiPointer capture. It must validate the current primary window first.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retirements_keep_window_identity_and_bound_overflow_without_inventing_a_window() {
        let mut world = World::new();
        world.init_resource::<Retirements>();
        let a = world.spawn_empty().id();
        let b = world.spawn_empty().id();
        retire(&mut world, Feature::Terminal, Some(a), &[7]);
        retire(&mut world, Feature::Companion, Some(b), &[7]);
        retire(&mut world, Feature::Terminal, None, &[99]);
        assert_eq!(world.resource::<Retirements>().contacts().collect::<Vec<_>>(), [(a, 7), (b, 7)]);
        assert!(world.resource::<Retirements>().changed(Feature::Terminal));
        assert!(world.resource::<Retirements>().changed(Feature::Companion));
        retire(&mut world, Feature::Terminal, Some(a), &(10..43).collect::<Vec<_>>());
        assert_eq!(world.resource::<Retirements>().contacts().count(), 32);
        assert!(world.resource::<Retirements>().overflowed());
        let taken = world.resource_mut::<Retirements>().take();
        assert!(taken.overflowed());
        assert_eq!(world.resource::<Retirements>().contacts().count(), 0);
        assert!(!world.resource::<Retirements>().changed(Feature::Terminal));
    }
    #[test]
    fn core_last_expires_barrier_once_without_taking_root_pending_mailbox() {
        let mut app = App::new();
        install(&mut app);
        install(&mut app); // Multiple public leaf registrations share one lifecycle plugin.
        assert!(app.is_plugin_added::<RetirementPlugin>());
        let window = app.world_mut().spawn_empty().id();
        retire(app.world_mut(), Feature::Terminal, Some(window), &[9]);
        assert!(app.world().resource::<Retirements>().barrier(Feature::Terminal));
        app.update();
        let pending = app.world().resource::<Retirements>();
        assert!(!pending.barrier(Feature::Terminal));
        assert!(pending.changed(Feature::Terminal));
        assert_eq!(pending.contacts().collect::<Vec<_>>(), [(window, 9)]);
        retire(app.world_mut(), Feature::Dialogue, None, &[10]);
        app.update();
        let pending = app.world().resource::<Retirements>();
        assert!(pending.changed(Feature::Terminal));
        assert!(pending.changed(Feature::Dialogue));
        assert!(!pending.barrier(Feature::Dialogue));
        assert_eq!(pending.contacts().collect::<Vec<_>>(), [(window, 9)]);
    }

}
