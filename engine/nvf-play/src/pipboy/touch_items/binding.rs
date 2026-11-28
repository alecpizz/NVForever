//! Explicit mobile assignment uses the same saved hotkey identity as drag/key input.
use super::*;

#[derive(Default)]
pub(super) struct Binding {
    slot: Option<usize>,
    candidate: Option<RowGuard>,
    label: String,
}
pub(super) fn available(world: &mut fnv_sim::World<'_>, candidate: Slot) -> bool {
    let (count, pick) = crate::hotkeys::carried(world, candidate);
    // Gameplay hotkeys deliberately fall back to another modification group.
    // Assignment must retain the exact, non-reserved identity selected by touch.
    let exact = world.peek(fnv_sim::FormId::PLAYER_REF).and_then(|state| state.inventory.as_ref())
        .is_some_and(|inventory| inventory.groups().any(|(id, extra, count)|
            id.0 == candidate.item && extra.mods == candidate.mods && !extra.live && count > 0));
    count > 0 && pick.mods == Some(candidate.mods) && exact
}

impl TouchItems {
    pub fn binding_slot(&self) -> Option<usize> { self.binding.slot }
    pub fn binding_ready(&self) -> bool { self.binding.candidate.is_some() }
    pub fn binding_label(&self) -> &str { &self.binding.label }
    pub fn arm_binding(&mut self, slot: usize) -> bool {
        self.cancel();
        if slot >= SLOTS || slot == AMMO_SWAP { return false; }
        self.binding.slot = Some(slot);
        self.message = format!("Slot {}: tap an inventory item, then confirm. No item will be used.", slot + 1);
        true
    }
    pub fn cancel_binding(&mut self) {
        self.binding = Binding::default();
        self.cancel();
        self.message = "Binding cancelled.".into();
    }
    pub fn choose_binding_again(&mut self) {
        self.binding.candidate = None;
        self.binding.label.clear();
        self.message = "Tap an inventory item, then confirm.".into();
    }
    pub(super) fn propose_binding(&mut self, active: &ActiveMenu) {
        if self.binding.slot.is_none() || self.binding.candidate.is_some()
            || !self.points.is_empty() || self.projected.is_some()
            || !self.contact.is_some_and(|c| c.ended && !c.moved && c.intent == Intent::Pending) {
            return;
        }
        let Some((menu, guard)) = active.0.as_ref().zip(self.row.take()).filter(|(m, g)| m.row_is(g)) else { return; };
        let row = &menu.list(Side::Player).rows[guard.index];
        if row.count <= 0 || row.live || row.folder.is_some() {
            self.message = "Choose an available inventory item.".into();
            return;
        }
        self.binding.candidate = Some(guard);
        self.binding.label = row.name.clone();
        self.message = format!("Bind {} to slot {}?", row.name, self.binding.slot.unwrap() + 1);
    }
    pub fn confirm_binding(&mut self, active: &ActiveMenu, hotkeys: &mut Hotkeys, world:&mut fnv_sim::World<'_>) -> bool {
        let Some((slot, guard)) = self.binding.slot.zip(self.binding.candidate) else { return false; };
        let Some(menu) = active.0.as_ref().filter(|m| m.row_is(&guard) && m.quantity.is_none()) else {
            self.choose_binding_again();
            self.message = "Inventory changed. Choose the item again.".into();
            return false;
        };
        let row = &menu.list(Side::Player).rows[guard.index];
        if row.count <= 0 || row.live || row.folder.is_some() {
            self.choose_binding_again();
            self.message = "That item is unavailable. Choose another item.".into();
            return false;
        }
        let candidate=Slot {item:row.item.0,mods:row.mods};
        if !available(world, candidate) {
            self.choose_binding_again();
            self.message="That item is no longer carried. Choose another item.".into();
            return false;
        }
        match hotkeys.assign(slot, candidate, row.kind) {
            Ok(_) => {
                let name = row.name.clone();
                self.cancel();
                self.message = format!("Slot {}: {}", slot + 1, name);
                true
            }
            Err(_) => {
                self.choose_binding_again();
                self.message = "This item cannot be assigned to that slot.".into();
                false
            }
        }
    }
}
