# Adaptive touch action presentation

[design: owner Android follow-up to #2436] The existing Aim control should say
**Block** for drawn melee weapons and fists, and **Scope** for a drawn scoped gun.
This increment changes presentation of that one action. Other contextual actions
remain separate follow-up work.

[documented: `nvf-play/src/combat.rs`, `control.rs`, `bindings.rs`, `aim_zoom.rs`]
`Control::Block` already supplies melee blocking and ranged aiming. Combat follows
the equipped weapon, including its mods and the runtime's Fists fallback. Its
melee classification is the classification used by the melee action system.
`player_weapon_scoped` requires ranged sights and an actual scope model, including
scope mods. The ready/drawn gates match the aiming system's `weapon_out` and
`WeaponAnim.ready` gates. No form IDs or weapon names are guessed here.

[design] While Playing with a ready drawn weapon: melee/Fists shows Block and the
owned `block` shield semantic glyph; an actually scope-equipped ranged weapon
shows Scope with the existing aim glyph; other weapons show Aim. Unknown state,
holstering/drawing and modal states use the generic Aim presentation. A persisted
**Adaptive touch actions** switch restores the static Aim caption/glyph when off.
This presentation reads the current combat state after equipment synchronization.

[design] The action remains `Control::Block`, at the same layout identity and
position. The legacy touch-controls.json key remains `Aim`. Captions are never
persistence keys. Existing button nodes and contacts are retained; a presentation
change neither presses/releases a binding nor changes aim/sneak toggle settings.
The original input owner remains responsible for release and modal/controller
cancellation. A held contact may therefore keep operating the same Block action
while its weapon capability changes; no second action is synthesized.

[documented: frozen icon-family SOURCE 935933efa9403047bbc122d73a71ad5e447d3af6d796ff8095e75193eb517398]
The renderer supplies semantic lookup independent of family/index and a shield
for `block` in all four families. This branch records that frozen source as a
dependency; its own delta is reviewed/applied after the family packet. Layout,
bindings and icons' physical sizing/tint/cache ownership stay independent.

[design] Tests must cover actual combat melee/Fists/scoped/modded transitions,
holstering/drawing/modal fallbacks and static preference persistence, then actual
touch/render-model routing with a held contact and hardware ownership preserved.
The combined touch-toggle/controller packet also needs a merged lifecycle test:
presentation must not replace nodes or prevent that owner's cancellation/release.

[qualification] The [R9 hosted CPU run](https://github.com/LootJunkieAzza/NVForever/actions/runs/43669164991/job/132675180379)
passed ten `touch::action_presentation` cases and two preference cases. They include
held/toggled input across presentation changes, independent mouse ownership,
controller handoff/consumed return, drawer cancellation and an actual pistol
visibility/raster/action control. This is CPU execution at R9 source `28d5469`,
not physical-device rendering or acceptance of later source changes.
