# Pip-Boy list scrollbar ownership

[documented: pipboy/touch_items.rs, tiles/inventory_menu.rs, tiles/list.rs]
The native marker initializes its drag on a press while held. Mobile row actions
normally press on a guarded Ended tap, so they cannot initialize a marker drag.
Also, queued Started/Ended points must each publish their own held state, rather
than the final contact's ended state.

[design] An actual painted native scrollbar marker hit claims sticky Control
intent at Started. It clears queued list-body scroll/assignment intent, keeps raw
projected movement and presses only that marker on Started. Subsequent movement
drives the existing native list drag; Ended releases it without a row/use click.
Rows and page/arrow actions retain their guarded Ended tap behavior. An actual
native tile classification, not a name or guessed screen region, authorizes this.
Session/modal/platform/device/hardware-priority cancellation remains authoritative.

[design] A scrollbar and the physical model wheel must reflect the list's current
scroll extent/value, independent of its selected item. Direct drag/page operations
change that same state; they never equip/use a row merely to move the thumb. The
hand-built view needs interactive targets for its actual painted marker/track,
with the same bounds and state as its drawing. A decorative bar is insufficient.

[design] `scrollbar::ListScrollRequest::new(pip, screen, ScrollBy)` pins the page,
tab, frame, image identity, projection mode and scale. `with_contact(id)` also pins
late per-contact quarantine. `apply(world)` rechecks the existing Admission and
modal/device gates, then updates the actual current list extent without moving its
selected gameplay row. Lines, pages and finite fractions all use that path.
Hand-built ITEMS owns a view scroll, as the native list does; normal hardware
selection navigation resumes selected-row following. STATS uses its existing top
row; DATA uses its actual first row and preserves it until selection navigation.
The physical wheel's rotation reads these scroll values rather than selected rows.

[design] Actual projected physical-knob contact geometry belongs to the separate
device-control adapter. It may queue this typed request with its owned raw touch
ID and a per-contact movement remainder. This leaf alone does not claim direct
physical-knob touch input or phone validation; that requires the composed adapter
and an actual input-to-model test. Native tile drag, hand-built hit targets and
real model-wheel readout are independently source-authored here.
