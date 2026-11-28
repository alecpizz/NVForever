# Mobile tray item binding

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

[design] On the inventory page, tap or hold a non-reserved tray slot to arm
binding. Tap an item, then use Confirm binding, Choose another item, or Cancel
binding. The chosen slot is marked BIND and the proposed item is named before
confirmation. Slot 2 remains Ammo Swap. The existing drag-to-slot path remains.

[documented: pipboy/touch_items/binding.rs] Arming does not emulate a held key or
mouse button. An armed native tap cannot dispatch the file's select/use action;
the hand-built page's actual touch click observer also rejects it. A short,
stationary contact may propose a captured inventory RowGuard. Vertical scrolling
does not propose an item. Confirmation checks the current row identity and
availability again, then calls the same Hotkeys.assign used by existing gameplay
controls, storing item identity and modifications. It does not use/equip the item.

[design] Contact/session, modal, device, focus and geometry cancellation retire
binding through the existing TouchItems cancellation path. Confirmation consumes
its candidate once; a changed or unavailable item requires a fresh choice. Empty
or unavailable assigned slots keep the existing hotkey behavior. Clear slot
remains in the slot options outside the armed flow.

[hypothesis: qualification] New actual native and hand-built pointer-route
regressions cover proposal without use, confirmation, stale-row rejection,
scroll arbitration and reserved-slot refusal. Authorship, compilation, CPU test
execution and physical acceptance are separate; this note makes no phone claim.
